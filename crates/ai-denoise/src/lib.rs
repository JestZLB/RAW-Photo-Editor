//! Local ONNX denoisers.
//!
//! The app does not bundle a model: users point it at a compatible ONNX file
//! (waifu2x denoise, RealCUGAN, SCUNet, …). Inference is pure Rust through
//! `tract`, runs on the CPU, and processes the image in overlapping tiles so
//! memory stays bounded on multi-megapixel photos.

use std::path::Path;

use image_core::DecodedImage;
use tract_onnx::prelude::*;

pub const TILE: usize = 192;
const OVERLAP: usize = 24;
const FEATHER: f32 = 24.0;

#[derive(Debug, thiserror::Error)]
pub enum DenoiseError {
    #[error("model could not be loaded: {0}")]
    Load(String),
    #[error("model failed to run: {0}")]
    Run(String),
    #[error("model must keep the tile size (denoise only, not upscaling): expected {expected:?}, got {actual:?}")]
    WrongOutputSize {
        expected: (usize, usize),
        actual: (usize, usize),
    },
    #[error("the image is empty")]
    EmptyImage,
}

pub struct DenoiseModel {
    plan: SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>,
}

impl DenoiseModel {
    /// Load and optimize an ONNX model whose input is `[1, 3, H, W]` float and
    /// whose output keeps the same spatial size.
    pub fn load(path: &Path) -> Result<Self, DenoiseError> {
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|error| DenoiseError::Load(error.to_string()))?;
        let fact =
            InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, TILE as i64, TILE as i64));
        let model = model
            .with_input_fact(0, fact)
            .map_err(|error| DenoiseError::Load(error.to_string()))?;
        let plan = model
            .into_optimized()
            .map_err(|error| DenoiseError::Load(error.to_string()))?
            .into_runnable()
            .map_err(|error| DenoiseError::Load(error.to_string()))?;
        Ok(Self { plan })
    }

    /// Denoise `image` and blend the result with the original by `strength`
    /// (0..1). RGB is processed; alpha is preserved.
    pub fn denoise(
        &self,
        image: &DecodedImage,
        strength: f32,
    ) -> Result<DecodedImage, DenoiseError> {
        let width = image.width as usize;
        let height = image.height as usize;
        if width == 0 || height == 0 {
            return Err(DenoiseError::EmptyImage);
        }
        let strength = strength.clamp(0.0, 1.0);
        let stride = TILE - OVERLAP;
        let mut accum = vec![0.0f32; width * height * 3];
        let mut weights = vec![0.0f32; width * height];

        let mut y0 = 0usize;
        loop {
            let mut x0 = 0usize;
            loop {
                let tile_h = TILE.min(height - y0);
                let tile_w = TILE.min(width - x0);
                let input = tile_input(image, x0, y0, tile_w, tile_h);
                let tensor = Tensor::from_shape(&[1, 3, TILE, TILE], &input)
                    .map_err(|error| DenoiseError::Run(error.to_string()))?;
                let outputs = self
                    .plan
                    .run(tvec!(tensor.into()))
                    .map_err(|error| DenoiseError::Run(error.to_string()))?;
                let output = &outputs[0];
                let shape = output.shape();
                if shape.len() != 4 || shape[2] != TILE || shape[3] != TILE {
                    let actual = if shape.len() == 4 {
                        (shape[2], shape[3])
                    } else {
                        (0, 0)
                    };
                    return Err(DenoiseError::WrongOutputSize {
                        expected: (TILE, TILE),
                        actual,
                    });
                }
                let data = output
                    .as_slice::<f32>()
                    .map_err(|error| DenoiseError::Run(error.to_string()))?;
                for ty in 0..tile_h {
                    for tx in 0..tile_w {
                        let px = x0 + tx;
                        let py = y0 + ty;
                        let weight = feather_weight(tx, ty, tile_w, tile_h);
                        let index = (py * width + px) * 3;
                        let tile_index = ty * TILE + tx;
                        accum[index] += data[tile_index] * weight;
                        accum[index + 1] += data[tile_index + TILE * TILE] * weight;
                        accum[index + 2] += data[tile_index + 2 * TILE * TILE] * weight;
                        weights[py * width + px] += weight;
                    }
                }
                if x0 + TILE >= width {
                    break;
                }
                x0 = (x0 + stride).min(width - 1);
            }
            if y0 + TILE >= height {
                break;
            }
            y0 = (y0 + stride).min(height - 1);
        }

        let mut out = Vec::with_capacity(width * height * 4);
        for (index, pixel) in image.rgba8.chunks_exact(4).enumerate() {
            let weight = weights[index].max(1e-6);
            for channel in 0..3 {
                let model_value = (accum[index * 3 + channel] / weight).clamp(0.0, 1.0);
                let original = pixel[channel] as f32 / 255.0;
                let blended = original + (model_value - original) * strength;
                out.push((blended * 255.0).round().clamp(0.0, 255.0) as u8);
            }
            out.push(pixel[3]);
        }
        DecodedImage::new(image.width, image.height, out)
            .map_err(|error| DenoiseError::Run(error.to_string()))
    }
}

/// Build one `[1,3,TILE,TILE]` NCHW tile in 0..1 sRGB, padding by clamping.
fn tile_input(
    image: &DecodedImage,
    x0: usize,
    y0: usize,
    tile_w: usize,
    tile_h: usize,
) -> Vec<f32> {
    let width = image.width as usize;
    let height = image.height as usize;
    let mut data = vec![0.0f32; 3 * TILE * TILE];
    for ty in 0..TILE {
        let sy = (y0 + ty).min(y0 + tile_h - 1).min(height - 1);
        for tx in 0..TILE {
            let sx = (x0 + tx).min(x0 + tile_w - 1).min(width - 1);
            let source = (sy * width + sx) * 4;
            let target = ty * TILE + tx;
            data[target] = image.rgba8[source] as f32 / 255.0;
            data[TILE * TILE + target] = image.rgba8[source + 1] as f32 / 255.0;
            data[2 * TILE * TILE + target] = image.rgba8[source + 2] as f32 / 255.0;
        }
    }
    data
}

fn feather_weight(x: usize, y: usize, tile_w: usize, tile_h: usize) -> f32 {
    let edge = |position: usize, length: usize| -> f32 {
        if length < 2 * OVERLAP {
            return 1.0;
        }
        let from_start = (position as f32 + 0.5) / FEATHER;
        let from_end = ((length - position) as f32 - 0.5) / FEATHER;
        from_start.min(from_end).clamp(0.0, 1.0)
    };
    let wx = edge(x, tile_w);
    let wy = edge(y, tile_h);
    // A zero weight would leave a seam when every overlapping tile fades out.
    (wx * wy).max(0.02)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::identity_onnx;

    fn gradient(width: u32, height: u32) -> DecodedImage {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x + y) % 256) as u8,
                    255,
                ]);
            }
        }
        DecodedImage::new(width, height, rgba).unwrap()
    }

    #[test]
    fn identity_model_round_trips_tiled_images() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.onnx");
        std::fs::write(&path, identity_onnx()).unwrap();
        let model = DenoiseModel::load(&path).expect("identity model loads");
        let image = gradient(500, 380);
        let denoised = model.denoise(&image, 1.0).unwrap();
        assert_eq!((denoised.width, denoised.height), (500, 380));
        for (before, after) in image
            .rgba8
            .chunks_exact(4)
            .zip(denoised.rgba8.chunks_exact(4))
        {
            for channel in 0..3 {
                assert!(
                    (before[channel] as i16 - after[channel] as i16).abs() <= 1,
                    "tile seams or colour drift at {before:?} -> {after:?}"
                );
            }
            assert_eq!(before[3], after[3]);
        }
    }

    #[test]
    fn strength_zero_leaves_pixels_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.onnx");
        std::fs::write(&path, identity_onnx()).unwrap();
        let model = DenoiseModel::load(&path).unwrap();
        let image = gradient(64, 48);
        let same = model.denoise(&image, 0.0).unwrap();
        assert_eq!(same.rgba8, image.rgba8);
    }

    #[test]
    fn missing_model_reports_a_load_error() {
        let error = DenoiseModel::load(Path::new("definitely-not-here.onnx"));
        assert!(matches!(error, Err(DenoiseError::Load(_))));
    }
}

/// Deterministic test model: a minimal ONNX graph that copies its input.
/// Used by integration tests so the inference path is exercised without a
/// downloaded checkpoint.
pub mod testing {
    /// Encode a minimal ONNX identity model with dynamic spatial dims.
    /// Hand-written protobuf keeps the test free of an ONNX build dependency.
    pub fn identity_onnx() -> Vec<u8> {
        fn varint(mut value: u64, out: &mut Vec<u8>) {
            loop {
                let byte = (value & 0x7f) as u8;
                value >>= 7;
                if value == 0 {
                    out.push(byte);
                    break;
                }
                out.push(byte | 0x80);
            }
        }
        fn key(field: u32, wire: u8, out: &mut Vec<u8>) {
            varint(((field as u64) << 3) | wire as u64, out);
        }
        fn bytes_field(field: u32, bytes: &[u8], out: &mut Vec<u8>) {
            key(field, 2, out);
            varint(bytes.len() as u64, out);
            out.extend_from_slice(bytes);
        }
        fn int_field(field: u32, value: u64, out: &mut Vec<u8>) {
            key(field, 0, out);
            varint(value, out);
        }
        fn dim(value: Option<u64>, symbol: Option<&str>) -> Vec<u8> {
            let mut out = Vec::new();
            if let Some(value) = value {
                int_field(1, value, &mut out);
            }
            if let Some(symbol) = symbol {
                bytes_field(2, symbol.as_bytes(), &mut out);
            }
            out
        }
        fn tensor_type(dims: &[Vec<u8>]) -> Vec<u8> {
            let mut tensor = Vec::new();
            int_field(1, 1, &mut tensor); // FLOAT
            let mut shape = Vec::new();
            for dimension in dims {
                bytes_field(1, dimension, &mut shape);
            }
            bytes_field(2, &shape, &mut tensor);
            let mut kind = Vec::new();
            bytes_field(1, &tensor, &mut kind);
            kind
        }
        fn value_info(name: &str, dims: &[Vec<u8>]) -> Vec<u8> {
            let mut out = Vec::new();
            bytes_field(1, name.as_bytes(), &mut out);
            bytes_field(2, &tensor_type(dims), &mut out);
            out
        }

        let dims = [
            dim(Some(1), None),
            dim(Some(3), None),
            dim(None, Some("h")),
            dim(None, Some("w")),
        ];
        let mut node = Vec::new();
        bytes_field(1, b"x", &mut node);
        bytes_field(2, b"y", &mut node);
        bytes_field(4, b"Identity", &mut node);

        let mut graph = Vec::new();
        bytes_field(1, &node, &mut graph);
        bytes_field(2, b"identity", &mut graph);
        bytes_field(11, &value_info("x", &dims), &mut graph);
        bytes_field(12, &value_info("y", &dims), &mut graph);

        let mut model = Vec::new();
        int_field(1, 7, &mut model); // ir_version
        bytes_field(2, b"luma-test", &mut model);
        bytes_field(7, &graph, &mut model);
        model
    }
}
