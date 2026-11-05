//! Optional export watermark compositing.
//!
//! The UI rasterizes text (or loads a logo) into a normal image, so this module
//! only has to scale and blend pixels. Watermarks are applied to the final
//! output size, after crop/resize and before the colour-space conversion, so
//! their colours are treated as sRGB working pixels like everything else.

use crate::{DecodedImage, ImageError, RasterImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

#[derive(Debug, Clone, Copy)]
pub struct WatermarkSpec<'a> {
    pub image: &'a DecodedImage,
    /// 0..1 blend applied on top of the watermark's own alpha.
    pub opacity: f32,
    /// Watermark width as a fraction of the output's shortest edge.
    pub scale: f32,
    /// Distance from the output edge as a fraction of the shortest edge.
    pub margin: f32,
    pub anchor: WatermarkAnchor,
}

/// Scale `spec.image` and alpha-composite it onto `raster` at the chosen anchor.
pub fn composite_watermark(
    raster: &mut RasterImage,
    spec: &WatermarkSpec<'_>,
) -> Result<(), ImageError> {
    let (width, height) = raster.dimensions();
    if width == 0 || height == 0 || spec.image.width == 0 || spec.image.height == 0 {
        return Ok(());
    }
    let opacity = if spec.opacity.is_finite() {
        spec.opacity.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if opacity <= 0.0 {
        return Ok(());
    }
    let scale = if spec.scale.is_finite() {
        spec.scale.clamp(0.01, 1.0)
    } else {
        0.18
    };
    let margin = if spec.margin.is_finite() {
        spec.margin.clamp(0.0, 0.5)
    } else {
        0.04
    };
    let short = width.min(height) as f32;
    let target_width = (short * scale).round().clamp(1.0, width as f32) as u32;
    let aspect = spec.image.height as f32 / spec.image.width as f32;
    let target_height = ((target_width as f32 * aspect).round() as u32)
        .clamp(1, height);
    let stamp = spec.image.resize_exact(target_width, target_height)?;
    let margin_px = (short * margin).round() as i64;
    let left = match spec.anchor {
        WatermarkAnchor::TopLeft | WatermarkAnchor::BottomLeft => margin_px,
        WatermarkAnchor::Center => (width as i64 - target_width as i64) / 2,
        WatermarkAnchor::TopRight | WatermarkAnchor::BottomRight => {
            width as i64 - target_width as i64 - margin_px
        }
    }
    .clamp(0, (width - target_width) as i64);
    let top = match spec.anchor {
        WatermarkAnchor::TopLeft | WatermarkAnchor::TopRight => margin_px,
        WatermarkAnchor::Center => (height as i64 - target_height as i64) / 2,
        WatermarkAnchor::BottomLeft | WatermarkAnchor::BottomRight => {
            height as i64 - target_height as i64 - margin_px
        }
    }
    .clamp(0, (height - target_height) as i64);

    match raster {
        RasterImage::Eight(image) => {
            blend_eight(image, &stamp, left as u32, top as u32, opacity);
        }
        RasterImage::Sixteen(image) => {
            blend_sixteen(image, &stamp, left as u32, top as u32, opacity);
        }
    }
    Ok(())
}

fn blend_eight(target: &mut DecodedImage, stamp: &DecodedImage, left: u32, top: u32, opacity: f32) {
    let target_width = target.width as usize;
    for y in 0..stamp.height {
        let dst_y = top + y;
        for x in 0..stamp.width {
            let dst_x = left + x;
            let src = ((y as usize * stamp.width as usize) + x as usize) * 4;
            let alpha = stamp.rgba8[src + 3] as f32 / 255.0 * opacity;
            if alpha <= 0.0 {
                continue;
            }
            let dst = (dst_y as usize * target_width + dst_x as usize) * 4;
            for channel in 0..3 {
                let mixed =
                    stamp.rgba8[src + channel] as f32 * alpha
                        + target.rgba8[dst + channel] as f32 * (1.0 - alpha);
                target.rgba8[dst + channel] = mixed.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn blend_sixteen(
    target: &mut crate::Srgb16Image,
    stamp: &DecodedImage,
    left: u32,
    top: u32,
    opacity: f32,
) {
    let target_width = target.width as usize;
    for y in 0..stamp.height {
        let dst_y = top + y;
        for x in 0..stamp.width {
            let dst_x = left + x;
            let src = ((y as usize * stamp.width as usize) + x as usize) * 4;
            let alpha = stamp.rgba8[src + 3] as f32 / 255.0 * opacity;
            if alpha <= 0.0 {
                continue;
            }
            let dst = (dst_y as usize * target_width + dst_x as usize) * 4;
            for channel in 0..3 {
                let source = stamp.rgba8[src + channel] as f32 * 257.0;
                let mixed = source * alpha + target.rgba[dst + channel] as f32 * (1.0 - alpha);
                target.rgba[dst + channel] = mixed.round().clamp(0.0, 65535.0) as u16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> DecodedImage {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            data.extend_from_slice(&rgba);
        }
        DecodedImage::new(width, height, data).unwrap()
    }

    #[test]
    fn watermark_lands_in_the_requested_corner() {
        let mut canvas = RasterImage::Eight(solid(4, 4, [255, 0, 0, 255]));
        let stamp = solid(2, 2, [255, 255, 255, 255]);
        composite_watermark(
            &mut canvas,
            &WatermarkSpec {
                image: &stamp,
                opacity: 1.0,
                scale: 0.5,
                margin: 0.0,
                anchor: WatermarkAnchor::BottomRight,
            },
        )
        .unwrap();
        let RasterImage::Eight(image) = canvas else {
            unreachable!()
        };
        assert_eq!(image.pixel(3, 3), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(2, 2), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
    }

    #[test]
    fn opacity_and_margins_are_applied() {
        let mut canvas = RasterImage::Eight(solid(10, 10, [0, 0, 0, 255]));
        let stamp = solid(2, 2, [255, 255, 255, 255]);
        composite_watermark(
            &mut canvas,
            &WatermarkSpec {
                image: &stamp,
                opacity: 0.5,
                scale: 0.2,
                margin: 0.1,
                anchor: WatermarkAnchor::TopLeft,
            },
        )
        .unwrap();
        let RasterImage::Eight(image) = canvas else {
            unreachable!()
        };
        assert_eq!(image.pixel(1, 1), Some([128, 128, 128, 255]));
        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_eq!(image.pixel(3, 3), Some([0, 0, 0, 255]));
    }

    #[test]
    fn sixteen_bit_targets_keep_full_range() {
        let mut canvas = RasterImage::Sixteen(crate::Srgb16Image::new(
            2,
            2,
            vec![0u16; 2 * 2 * 4],
        )
        .unwrap());
        let stamp = solid(2, 2, [255, 255, 255, 255]);
        composite_watermark(
            &mut canvas,
            &WatermarkSpec {
                image: &stamp,
                opacity: 1.0,
                scale: 1.0,
                margin: 0.0,
                anchor: WatermarkAnchor::Center,
            },
        )
        .unwrap();
        let RasterImage::Sixteen(image) = canvas else {
            unreachable!()
        };
        assert_eq!(image.rgba[0], 65535);
        assert_eq!(image.rgba[3], 0);
    }
}
