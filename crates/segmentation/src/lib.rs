//! User-supplied ONNX segmentation adapter.
//!
//! The model contract is deliberately narrow: `[1, 3, 512, 512]` NCHW float RGB
//! in 0..1, output a spatial confidence map (`[1,1,H,W]`, `[1,2,H,W]` where the
//! second channel is foreground, or a bare `[H,W]`). Output is tiled with
//! overlapping windows and feathered, then returned as `Coverage` for the
//! document's raster mask (ADR-014).
//!
//! Inference is pure Rust via `tract` on the CPU. No weights are shipped: the
//! caller supplies the `.onnx` path, and a failed probe is reported as such.

use std::path::Path;

use image_core::DecodedImage;
use tract_onnx::prelude::*;

/// Input tile edge. Fixed because the input fact is pinned to this size; models
/// with a different fixed input are rejected with a clear error instead of
/// silently producing a wrong mask.
pub const TILE: usize = 512;
/// Overlap between tiles, in pixels. Wide enough to hide seam artefacts on soft
/// subject edges without making the tile count explode.
const OVERLAP: usize = 48;

#[derive(Debug, thiserror::Error)]
pub enum SegmentationError {
    #[error("failed to load model: {0}")]
    Load(String),
    #[error("model inference failed: {0}")]
    Run(String),
    #[error("model input is not [1,3,{TILE},{TILE}] float: {0}")]
    WrongInput(String),
    #[error("model output shape is not supported: {0}")]
    WrongOutput(String),
    #[error("image is empty")]
    EmptyImage,
}

impl SegmentationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Load(_) | Self::WrongInput(_) | Self::WrongOutput(_) => {
                "SEGMENTATION_MODEL_INVALID"
            }
            Self::Run(_) => "SEGMENTATION_FAILED",
            Self::EmptyImage => "IMAGE_INVALID",
        }
    }

    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Load(_) | Self::WrongInput(_) | Self::WrongOutput(_) => {
                "这个分割模型无法使用：需要输入 512×512 的 RGB 图片，并输出同尺寸的置信度图。"
            }
            Self::Run(_) => "分割模型运行失败，请换一个模型或稍后重试。",
            Self::EmptyImage => "这张照片没有可用的像素。",
        }
    }
}

/// Per-pixel coverage in 0..1, row-major, spanning the uncropped source.
#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

pub struct SegmentationModel {
    plan: SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>,
}

impl SegmentationModel {
    pub fn load(path: &Path) -> Result<Self, SegmentationError> {
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|error| SegmentationError::Load(error.to_string()))?;
        let fact =
            InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, TILE as i64, TILE as i64));
        let model = model
            .with_input_fact(0, fact)
            .map_err(|error| SegmentationError::WrongInput(error.to_string()))?;
        let plan = model
            .into_optimized()
            .map_err(|error| SegmentationError::Load(error.to_string()))?
            .into_runnable()
            .map_err(|error| SegmentationError::Load(error.to_string()))?;
        Ok(Self { plan })
    }

    /// Run the model over `image` and return the assembled confidence map.
    pub fn segment(&self, image: &DecodedImage) -> Result<Coverage, SegmentationError> {
        let width = image.width as usize;
        let height = image.height as usize;
        if width == 0 || height == 0 {
            return Err(SegmentationError::EmptyImage);
        }
        let mut accum = vec![0.0f32; width * height];
        let mut weights = vec![0.0f32; width * height];

        for (y0, x0) in tile_origins(width, height) {
            let tile_w = TILE.min(width - x0);
            let tile_h = TILE.min(height - y0);
            let input = tile_input(image, x0, y0);
            let tensor = Tensor::from_shape(&[1, 3, TILE, TILE], &input)
                .map_err(|error| SegmentationError::Run(error.to_string()))?;
            let outputs = self
                .plan
                .run(tvec!(tensor.into()))
                .map_err(|error| SegmentationError::Run(error.to_string()))?;
            let output = &outputs[0];
            let foreground = take_foreground(
                output
                    .as_slice::<f32>()
                    .map_err(|error| SegmentationError::Run(error.to_string()))?,
                output.shape(),
            )?;
            for ty in 0..tile_h {
                for tx in 0..tile_w {
                    let weight = feather_weight(tx, ty, tile_w, tile_h);
                    let target = (y0 + ty) * width + x0 + tx;
                    accum[target] += foreground[ty * TILE + tx] * weight;
                    weights[target] += weight;
                }
            }
        }

        let mut values = vec![0.0f32; width * height];
        for (index, value) in values.iter_mut().enumerate() {
            let weight = weights[index].max(1e-6);
            *value = (accum[index] / weight).clamp(0.0, 1.0);
        }
        Ok(Coverage {
            width: image.width,
            height: image.height,
            values,
        })
    }
}

/// Top-left corners of every tile needed to cover `width × height`, row-major.
fn tile_origins(width: usize, height: usize) -> Vec<(usize, usize)> {
    let axis = |length: usize| -> Vec<usize> {
        let mut out = vec![0usize];
        if length <= TILE {
            return out;
        }
        let stride = TILE - OVERLAP;
        let mut next = 0usize;
        loop {
            next = (next + stride).min(length - TILE);
            out.push(next);
            if next + TILE >= length {
                break;
            }
        }
        out
    };
    let xs = axis(width);
    let ys = axis(height);
    let mut origins = Vec::with_capacity(xs.len() * ys.len());
    for y in ys {
        for x in &xs {
            origins.push((y, *x));
        }
    }
    origins
}

/// `[1,3,TILE,TILE]` NCHW tile in 0..1 sRGB, clamped at the image edge.
fn tile_input(image: &DecodedImage, x0: usize, y0: usize) -> Vec<f32> {
    let width = image.width as usize;
    let height = image.height as usize;
    let mut out = vec![0.0f32; 3 * TILE * TILE];
    for ty in 0..TILE {
        let sy = (y0 + ty).min(height - 1);
        for tx in 0..TILE {
            let sx = (x0 + tx).min(width - 1);
            let source = (sy * width + sx) * 4;
            let target = ty * TILE + tx;
            for channel in 0..3 {
                out[channel * TILE * TILE + target] =
                    f32::from(image.rgba8[source + channel]) / 255.0;
            }
        }
    }
    out
}

/// Extract the foreground channel from a model output.
///
/// Accepts `[1,1,H,W]`, `[1,C,H,W]` (channel 1 is foreground), `[1,H,W]`,
/// `[C,H,W]` and `[H,W]`. Values are activated only when the model returned
/// logits, so a model that already emits probabilities is not squashed twice.
fn take_foreground(data: &[f32], shape: &[usize]) -> Result<Vec<f32>, SegmentationError> {
    let (channel, height, width) = match shape {
        [1, channels, height, width] => (foreground_channel(*channels)?, *height, *width),
        [channels, height, width] => (foreground_channel(*channels)?, *height, *width),
        [height, width] => (0, *height, *width),
        _ => return Err(SegmentationError::WrongOutput(format!("{shape:?}"))),
    };
    let plane = height * width;
    let start = channel * plane;
    let slice = data.get(start..start + plane).ok_or_else(|| {
        SegmentationError::WrongOutput(format!("{shape:?} from {} values", data.len()))
    })?;
    let mut values = slice.to_vec();
    activate(&mut values);
    Ok(values)
}

fn foreground_channel(channels: usize) -> Result<usize, SegmentationError> {
    match channels {
        1 => Ok(0),
        2 => Ok(1),
        other => Err(SegmentationError::WrongOutput(format!(
            "{other} channels (expected 1 or 2)"
        ))),
    }
}

/// Map logits to 0..1. A model that already outputs probabilities is left alone;
/// the check uses the value range rather than the tensor dtype.
fn activate(values: &mut [f32]) {
    // Non-finite values carry no information; drop them before deciding whether
    // the tensor holds logits or probabilities.
    for value in values.iter_mut() {
        if !value.is_finite() {
            *value = 0.0;
        }
    }
    let logits = values.iter().any(|value| *value < -0.001 || *value > 1.001);
    if !logits {
        for value in values.iter_mut() {
            *value = value.clamp(0.0, 1.0);
        }
        return;
    }
    for value in values.iter_mut() {
        *value = 1.0 / (1.0 + (-*value).exp());
    }
}

/// Triangular window so overlapping tiles blend instead of showing seams.
fn feather_weight(x: usize, y: usize, width: usize, height: usize) -> f32 {
    let axis = |position: usize, length: usize| -> f32 {
        if length <= 1 {
            return 1.0;
        }
        let normalized = (position as f32 + 0.5) / length as f32;
        (1.0 - (2.0 * normalized - 1.0).abs()).clamp(0.05, 1.0)
    };
    axis(x, width) * axis(y, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_cover_small_images_once_and_large_ones_with_overlap() {
        assert_eq!(tile_origins(100, 80), vec![(0, 0)]);
        // The last tile is pulled flush to the edge instead of running past it,
        // so 700-512 = 188 overlaps the first tile.
        assert_eq!(tile_origins(700, 400), vec![(0, 0), (0, 188)]);
        let nine = tile_origins(1400, 1400);
        assert_eq!(nine.len(), 9, "three tiles per axis for 1400px");

        // Every tile must stay inside the image.
        for (y, x) in nine.iter().copied() {
            assert!(x + TILE <= 1400 && y + TILE <= 1400);
        }
        let xs: Vec<usize> = nine.iter().map(|(_, x)| *x).take(3).collect();
        assert_eq!(xs, vec![0, 464, 888]);
        assert!(xs[0] + TILE >= xs[1], "tiles must overlap, not leave gaps");
    }

    #[test]
    fn tile_input_pads_outside_the_image_with_edge_pixels() {
        let image = DecodedImage::new(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]).unwrap();
        let input = tile_input(&image, 0, 0);
        assert!((input[0] - 10.0 / 255.0).abs() < 1e-6);
        // Last column clamps to x = 1.
        assert!((input[TILE - 1] - 40.0 / 255.0).abs() < 1e-6);
        // Row padding clamps to the only row: channel 0, row 1, column 0.
        assert!((input[TILE] - 10.0 / 255.0).abs() < 1e-6);
        // Channel 1 of the same pixel is the green sample.
        assert!((input[TILE * TILE] - 20.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn single_channel_probabilities_are_used_as_is() {
        let values = vec![0.1, 0.6, 0.9, 0.4];
        assert_eq!(take_foreground(&values, &[1, 1, 2, 2]).unwrap(), values);
    }

    #[test]
    fn two_channel_output_takes_the_foreground_channel() {
        let mut values = vec![0.9, 0.8, 0.7, 0.6];
        values.extend_from_slice(&[0.05, 0.95, 0.1, 0.85]);
        assert_eq!(
            take_foreground(&values, &[1, 2, 2, 2]).unwrap(),
            vec![0.05, 0.95, 0.1, 0.85]
        );
        // Same layout without the batch dimension.
        assert_eq!(
            take_foreground(&values, &[2, 2, 2]).unwrap(),
            vec![0.05, 0.95, 0.1, 0.85]
        );
    }

    #[test]
    fn bare_spatial_output_is_accepted() {
        let values = vec![0.2, 0.7];
        assert_eq!(take_foreground(&values, &[1, 2]).unwrap(), values);
        assert_eq!(take_foreground(&values, &[1, 1, 2]).unwrap(), values);
    }

    #[test]
    fn unsupported_layouts_are_rejected() {
        let error = take_foreground(&[0.0; 3 * 4], &[1, 3, 2, 2]).unwrap_err();
        assert_eq!(error.code(), "SEGMENTATION_MODEL_INVALID");

        // Shape promises more values than the buffer holds.
        let error = take_foreground(&[0.0; 2], &[1, 1, 2, 2]).unwrap_err();
        assert_eq!(error.code(), "SEGMENTATION_MODEL_INVALID");
    }

    #[test]
    fn logits_are_squashed_but_probabilities_are_not() {
        let mut probabilities = vec![0.2, 0.8];
        activate(&mut probabilities);
        assert_eq!(probabilities, vec![0.2, 0.8]);

        let mut logits = vec![-2.0, 2.0];
        activate(&mut logits);
        assert!((logits[0] - 0.1192).abs() < 1e-3);
        assert!((logits[1] - 0.8808).abs() < 1e-3);

        let mut with_nan = vec![f32::NAN, 0.5];
        activate(&mut with_nan);
        assert_eq!(with_nan[0], 0.0);
        assert_eq!(with_nan[1], 0.5);
    }

    #[test]
    fn feather_weights_stay_positive_and_favour_tile_centres() {
        let centre = feather_weight(256, 256, 512, 512);
        let edge = feather_weight(0, 0, 512, 512);
        assert!(edge > 0.0);
        assert!(centre > edge);
    }
}
