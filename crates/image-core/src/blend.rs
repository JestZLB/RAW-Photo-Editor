use rayon::prelude::*;

use crate::{ImageError, LinearImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
}

fn blend_channel(mode: BlendMode, base: f32, blend: f32) -> f32 {
    match mode {
        BlendMode::Normal => blend,
        BlendMode::Multiply => base.clamp(0.0, 1.0) * blend.clamp(0.0, 1.0),
        BlendMode::Screen => {
            let b = base.clamp(0.0, 1.0);
            let s = blend.clamp(0.0, 1.0);
            b + s - b * s
        }
        BlendMode::Overlay => {
            let b = base.clamp(0.0, 1.0);
            let s = blend.clamp(0.0, 1.0);
            if b <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        BlendMode::SoftLight => {
            let b = base.clamp(0.0, 1.0);
            let s = blend.clamp(0.0, 1.0);
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
    }
}

/// Alpha-aware composite of `src` over `dst` with opacity. RGB values are in
/// linear light. `opacity <= 0` is an exact no-op.
pub fn composite_over(
    dst: &mut LinearImage,
    src: &LinearImage,
    opacity: f32,
    mode: BlendMode,
) -> Result<(), ImageError> {
    if dst.width != src.width || dst.height != src.height {
        return Err(ImageError::InvalidImage(format!(
            "composite size mismatch: {}x{} vs {}x{}",
            dst.width, dst.height, src.width, src.height
        )));
    }
    if !opacity.is_finite() || opacity <= 0.0 {
        return Ok(());
    }
    let opacity = opacity.clamp(0.0, 1.0);
    dst.data
        .par_chunks_mut(4)
        .zip(src.data.par_chunks(4))
        .for_each(|(d, s)| {
            let sa = if s[3].is_finite() {
                s[3].clamp(0.0, 1.0) * opacity
            } else {
                0.0
            };
            if sa <= 0.0 {
                return;
            }
            let da = if d[3].is_finite() {
                d[3].clamp(0.0, 1.0)
            } else {
                0.0
            };
            let oa = sa + da * (1.0 - sa);
            if oa <= 0.0 {
                d[0] = 0.0;
                d[1] = 0.0;
                d[2] = 0.0;
                d[3] = 0.0;
                return;
            }
            for c in 0..3 {
                let blended = if da <= 1e-6 {
                    s[c]
                } else {
                    blend_channel(mode, d[c], s[c])
                };
                let value = (blended * sa + d[c] * da * (1.0 - sa)) / oa;
                d[c] = if value.is_finite() { value } else { 0.0 };
            }
            d[3] = oa;
        });
    Ok(())
}

/// Alpha-aware composite restricted by a per-pixel mask (0..1). The mask is
/// normalized to the destination dimensions and multiplies layer opacity.
pub fn composite_over_masked(
    dst: &mut LinearImage,
    src: &LinearImage,
    mask: &[f32],
    opacity: f32,
    mode: BlendMode,
) -> Result<(), ImageError> {
    if dst.width != src.width || dst.height != src.height {
        return Err(ImageError::InvalidImage(format!(
            "composite size mismatch: {}x{} vs {}x{}",
            dst.width, dst.height, src.width, src.height
        )));
    }
    if mask.len() != dst.width as usize * dst.height as usize {
        return Err(ImageError::InvalidImage(
            "mask dimensions do not match the image".into(),
        ));
    }
    if !opacity.is_finite() || opacity <= 0.0 {
        return Ok(());
    }
    let opacity = opacity.clamp(0.0, 1.0);
    dst.data
        .par_chunks_mut(4)
        .zip(src.data.par_chunks(4))
        .zip(mask.par_iter())
        .for_each(|((d, s), mask)| {
            let coverage = if mask.is_finite() {
                mask.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let sa = if s[3].is_finite() {
                s[3].clamp(0.0, 1.0) * opacity * coverage
            } else {
                0.0
            };
            if sa <= 0.0 {
                return;
            }
            let da = if d[3].is_finite() {
                d[3].clamp(0.0, 1.0)
            } else {
                0.0
            };
            let oa = sa + da * (1.0 - sa);
            if oa <= 0.0 {
                d[0] = 0.0;
                d[1] = 0.0;
                d[2] = 0.0;
                d[3] = 0.0;
                return;
            }
            for c in 0..3 {
                let blended = if da <= 1e-6 {
                    s[c]
                } else {
                    blend_channel(mode, d[c], s[c])
                };
                let value = (blended * sa + d[c] * da * (1.0 - sa)) / oa;
                d[c] = if value.is_finite() { value } else { 0.0 };
            }
            d[3] = oa;
        });
    Ok(())
}

/// Separable box blur. Used for clarity local contrast, portrait frequency
/// separation and skin-mask feathering. Alpha is blurred too; callers that need
/// to preserve alpha restore it after the blur.
pub fn box_blur(image: &LinearImage, radius: u32) -> LinearImage {
    if radius == 0 {
        return image.clone();
    }
    let horizontal = blur_horizontal(image, radius as usize);
    let transposed = transpose(&horizontal);
    let blurred = blur_horizontal(&transposed, radius as usize);
    transpose(&blurred)
}

pub fn transpose(image: &LinearImage) -> LinearImage {
    let width = image.width as usize;
    let height = image.height as usize;
    let mut data = vec![0.0f32; image.data.len()];
    // The transposed image has `width` rows of `height` pixels each. Row length
    // must therefore be `height * 4`, not `width * 4`.
    data.par_chunks_mut(height * 4)
        .enumerate()
        .for_each(|(x, out_row)| {
            for y in 0..height {
                let src = (y * width + x) * 4;
                let dst = y * 4;
                out_row[dst..dst + 4].copy_from_slice(&image.data[src..src + 4]);
            }
        });
    LinearImage {
        width: image.height,
        height: image.width,
        data,
    }
}

fn blur_horizontal(image: &LinearImage, radius: usize) -> LinearImage {
    let width = image.width as usize;
    let radius = radius.min(width.saturating_sub(1));
    let mut data = vec![0.0f32; image.data.len()];
    data.par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(y, out_row)| {
            let row = &image.data[y * width * 4..(y + 1) * width * 4];
            let mut prefix = vec![[0.0f32; 4]; width + 1];
            for x in 0..width {
                let src = x * 4;
                let prev = prefix[x];
                prefix[x + 1] = [
                    prev[0] + row[src],
                    prev[1] + row[src + 1],
                    prev[2] + row[src + 2],
                    prev[3] + row[src + 3],
                ];
            }
            for x in 0..width {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(width - 1);
                let count = (hi - lo + 1) as f32;
                let a = prefix[lo];
                let b = prefix[hi + 1];
                let dst = x * 4;
                out_row[dst] = (b[0] - a[0]) / count;
                out_row[dst + 1] = (b[1] - a[1]) / count;
                out_row[dst + 2] = (b[2] - a[2]) / count;
                out_row[dst + 3] = (b[3] - a[3]) / count;
            }
        });
    LinearImage {
        width: image.width,
        height: image.height,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> LinearImage {
        let mut data = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                data.extend_from_slice(&[x as f32 / 7.0, y as f32 / 7.0, 0.2, 1.0]);
            }
        }
        LinearImage::new(8, 8, data).unwrap()
    }

    #[test]
    fn composite_opacity_scales_alpha() {
        let mut dst = gradient();
        let src = LinearImage::new(8, 8, vec![1.0; 8 * 8 * 4]).unwrap();
        composite_over(&mut dst, &src, 0.5, BlendMode::Normal).unwrap();
        assert!((dst.data[0] - (1.0 * 0.5 + 0.0 * 0.5)).abs() < 1e-5);
        assert!((dst.data[3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn composite_transparent_backdrop_shows_source() {
        let mut dst = LinearImage::transparent(4, 4).unwrap();
        let src = LinearImage::new(4, 4, [0.25, 0.5, 0.75, 1.0].repeat(16)).unwrap();
        composite_over(&mut dst, &src, 1.0, BlendMode::Multiply).unwrap();
        assert!((dst.data[0] - 0.25).abs() < 1e-5);
        assert!((dst.data[1] - 0.5).abs() < 1e-5);
        assert!((dst.data[2] - 0.75).abs() < 1e-5);
    }

    #[test]
    fn masked_composite_only_changes_painted_pixels() {
        let mut dst = gradient();
        let src = LinearImage::new(8, 8, vec![1.0; 8 * 8 * 4]).unwrap();
        let mut mask = vec![0.0f32; 64];
        for y in 0..8 {
            for x in 0..4 {
                mask[y * 8 + x] = 1.0;
            }
        }
        let before = dst.clone();
        composite_over_masked(&mut dst, &src, &mask, 1.0, BlendMode::Normal).unwrap();
        for y in 0..8 {
            for x in 0..8 {
                let px = dst.pixel(x, y).unwrap();
                let original = before.pixel(x, y).unwrap();
                if x < 4 {
                    assert!((px[0] - 1.0).abs() < 1e-5);
                    assert!((px[3] - 1.0).abs() < 1e-5);
                } else {
                    assert_eq!(px, original);
                }
            }
        }
    }

    #[test]
    fn transpose_handles_non_square_images() {
        let mut data = Vec::new();
        for y in 0..3u32 {
            for x in 0..5u32 {
                data.extend_from_slice(&[x as f32, y as f32, 0.0, 1.0]);
            }
        }
        let image = LinearImage::new(5, 3, data).unwrap();
        let transposed = transpose(&image);
        assert_eq!((transposed.width, transposed.height), (3, 5));
        let pixel = transposed.pixel(1, 4).unwrap();
        assert_eq!(pixel[0], 4.0);
        assert_eq!(pixel[1], 1.0);
        let round_trip = transpose(&transposed);
        assert_eq!(round_trip, image);
    }

    #[test]
    fn blur_smooths_impulse() {
        let mut linear = LinearImage::transparent(5, 5).unwrap();
        let center = linear.index(2, 2).unwrap();
        linear.data[center] = 1.0;
        let blurred = box_blur(&linear, 1);
        assert!(blurred.data[center] < 1.0);
        assert!(blurred.data[center] > 0.0);
        let total_before: f32 = linear.data.iter().step_by(4).sum();
        let total_after: f32 = blurred.data.iter().step_by(4).sum();
        assert!((total_before - total_after).abs() < 1e-4);
    }
}
