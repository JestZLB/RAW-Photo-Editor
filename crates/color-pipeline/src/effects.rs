//! Spatial effects: denoise, dehaze, chromatic correction, vignette and grain.
//!
//! Everything runs on the linear working image. The operators are deliberately
//! simple and deterministic; each one is a no-op at strength zero so the
//! renderer can skip them entirely.

use image_core::LinearImage;
use rayon::prelude::*;

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Separable, edge-clamped box blur of a single plane.
fn blur_plane(values: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || width == 0 || height == 0 {
        return values.to_vec();
    }
    let window = (radius * 2 + 1) as f32;
    let mut horizontal = vec![0.0f32; values.len()];
    for y in 0..height {
        let row = y * width;
        let mut sum = 0.0f32;
        for x in 0..width {
            if x == 0 {
                for offset in 0..=radius * 2 {
                    let sample = offset.saturating_sub(radius).min(width - 1);
                    sum += values[row + sample];
                }
            } else {
                let leaving =
                    (x as isize - radius as isize - 1).clamp(0, width as isize - 1) as usize;
                let entering = (x + radius).min(width - 1);
                sum += values[row + entering] - values[row + leaving];
            }
            horizontal[row + x] = sum / window;
        }
    }
    let mut out = vec![0.0f32; values.len()];
    for x in 0..width {
        let mut sum = 0.0f32;
        for y in 0..height {
            if y == 0 {
                for offset in 0..=radius * 2 {
                    let sample = offset.saturating_sub(radius).min(height - 1);
                    sum += horizontal[sample * width + x];
                }
            } else {
                let leaving =
                    (y as isize - radius as isize - 1).clamp(0, height as isize - 1) as usize;
                let entering = (y + radius).min(height - 1);
                sum += horizontal[entering * width + x] - horizontal[leaving * width + x];
            }
            out[y * width + x] = sum / window;
        }
    }
    out
}

/// Edge-preserving luminance denoise plus chroma blur.
///
/// Luma uses a small bilateral filter (spatial and range weights on luminance),
/// so flat skin smooths while edges keep their contrast. Chroma is blurred over
/// a wider radius because colour noise is low-frequency and invisible in luma.
pub fn apply_noise_reduction(image: &mut LinearImage, luma_strength: f32, chroma_strength: f32) {
    let luma_amount = (luma_strength / 100.0).clamp(0.0, 1.0);
    let chroma_amount = (chroma_strength / 100.0).clamp(0.0, 1.0);
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || (luma_amount <= 0.0 && chroma_amount <= 0.0) {
        return;
    }

    if chroma_amount > 0.0 {
        let radius = (1.0 + chroma_amount * 4.0).round() as usize;
        let mut cb = Vec::with_capacity(width * height);
        let mut cr = Vec::with_capacity(width * height);
        for pixel in image.data.chunks_exact(4) {
            let y = luma(pixel[0], pixel[1], pixel[2]);
            cb.push(pixel[2] - y);
            cr.push(pixel[0] - y);
        }
        let cb = blur_plane(&cb, width, height, radius);
        let cr = blur_plane(&cr, width, height, radius);
        for (index, pixel) in image.data.chunks_exact_mut(4).enumerate() {
            let y = luma(pixel[0], pixel[1], pixel[2]);
            let cb_new = (pixel[2] - y) * (1.0 - chroma_amount) + cb[index] * chroma_amount;
            let cr_new = (pixel[0] - y) * (1.0 - chroma_amount) + cr[index] * chroma_amount;
            let b = y + cb_new;
            let r = y + cr_new;
            let g = (y - 0.2126 * r - 0.0722 * b) / 0.7152;
            pixel[0] = r.max(0.0);
            pixel[1] = g.max(0.0);
            pixel[2] = b.max(0.0);
        }
    }

    if luma_amount > 0.0 {
        let radius = (1.0 + luma_amount * 2.0).round() as isize;
        let sigma_space = radius as f32;
        let sigma_range = 0.02 + luma_amount * 0.1;
        let plane: Vec<f32> = image
            .data
            .chunks_exact(4)
            .map(|pixel| luma(pixel[0], pixel[1], pixel[2]))
            .collect();
        let filtered = plane
            .par_iter()
            .enumerate()
            .map(|(index, center)| {
                let x = index % width;
                let y = index / width;
                let mut sum = 0.0f32;
                let mut weight_sum = 0.0f32;
                for dy in -radius..=radius {
                    let sy = (y as isize + dy).clamp(0, height as isize - 1) as usize;
                    for dx in -radius..=radius {
                        let sx = (x as isize + dx).clamp(0, width as isize - 1) as usize;
                        let neighbour = plane[sy * width + sx];
                        let spatial = (-((dx * dx + dy * dy) as f32)
                            / (2.0 * sigma_space * sigma_space))
                            .exp();
                        let range = (-((neighbour - center).powi(2))
                            / (2.0 * sigma_range * sigma_range))
                            .exp();
                        let weight = spatial * range;
                        sum += neighbour * weight;
                        weight_sum += weight;
                    }
                }
                if weight_sum > 0.0 {
                    sum / weight_sum
                } else {
                    *center
                }
            })
            .collect::<Vec<f32>>();

        for (index, pixel) in image.data.chunks_exact_mut(4).enumerate() {
            let y = luma(pixel[0], pixel[1], pixel[2]);
            if y <= 1e-5 {
                continue;
            }
            let target = y + (filtered[index] - y) * luma_amount;
            let scale = (target / y).max(0.0);
            pixel[0] *= scale;
            pixel[1] *= scale;
            pixel[2] *= scale;
        }
    }
}

/// Dark-channel dehaze. The transmission map is estimated from a blurred dark
/// channel and blended toward 1.0 by the strength, so low values stay gentle.
pub fn apply_dehaze(image: &mut LinearImage, amount: f32) {
    let strength = (amount / 100.0).clamp(0.0, 1.0);
    let width = image.width as usize;
    let height = image.height as usize;
    if strength <= 0.0 || width == 0 || height == 0 {
        return;
    }
    let count = width * height;
    let mut dark = Vec::with_capacity(count);
    let mut lumas = Vec::with_capacity(count);
    for pixel in image.data.chunks_exact(4) {
        dark.push(pixel[0].min(pixel[1]).min(pixel[2]));
        lumas.push(luma(pixel[0], pixel[1], pixel[2]));
    }
    let radius = ((width.min(height) as f32) / 50.0).round().clamp(2.0, 40.0) as usize;
    let dark = blur_plane(&dark, width, height, radius);

    lumas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile = (((lumas.len() - 1) as f32) * 0.95).round() as usize;
    let atmospheric = lumas[percentile.min(lumas.len() - 1)].clamp(0.05, 1.0);
    let transmission: Vec<f32> = dark
        .iter()
        .map(|value| (1.0 - 0.9 * (value / atmospheric)).clamp(0.15, 1.0))
        .collect();
    let transmission = blur_plane(&transmission, width, height, radius);

    for (index, pixel) in image.data.chunks_exact_mut(4).enumerate() {
        let t = 1.0 + (transmission[index] - 1.0) * strength;
        let safe = t.max(0.15);
        for channel in pixel.iter_mut().take(3) {
            *channel = ((*channel - atmospheric) / safe + atmospheric).max(0.0);
        }
    }
}

/// Radial red/blue scaling around the centre, for manual fringe correction.
pub fn apply_chromatic(image: &mut LinearImage, amount: f32) {
    let strength = (amount / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 || image.width < 2 || image.height < 2 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    let scale = 0.002 * strength;
    let source = image.data.clone();
    let cx = (width as f32 - 1.0) * 0.5;
    let cy = (height as f32 - 1.0) * 0.5;
    let sample = |x: f32, y: f32, channel: usize| -> f32 {
        let x = x.clamp(0.0, width as f32 - 1.0);
        let y = y.clamp(0.0, height as f32 - 1.0);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(width - 1);
        let y1 = (y0 + 1).min(height - 1);
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let a = source[(y0 * width + x0) * 4 + channel];
        let b = source[(y0 * width + x1) * 4 + channel];
        let c = source[(y1 * width + x0) * 4 + channel];
        let d = source[(y1 * width + x1) * 4 + channel];
        (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy
    };
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let red_x = cx + dx * (1.0 - scale);
            let red_y = cy + dy * (1.0 - scale);
            let blue_x = cx + dx * (1.0 + scale);
            let blue_y = cy + dy * (1.0 + scale);
            let index = (y * width + x) * 4;
            image.data[index] = sample(red_x, red_y, 0);
            image.data[index + 2] = sample(blue_x, blue_y, 2);
        }
    }
}

/// Radial vignette: positive darkens the corners, negative lifts them.
pub fn apply_vignette(image: &mut LinearImage, amount: f32) {
    let strength = (amount / 100.0).clamp(-1.0, 1.0);
    if strength.abs() < 1e-4 || image.width == 0 || image.height == 0 {
        return;
    }
    let width = image.width as usize;
    let cx = (image.width as f32 - 1.0) * 0.5;
    let cy = (image.height as f32 - 1.0) * 0.5;
    let max_radius = (cx * cx + cy * cy).sqrt().max(1.0);
    for (index, pixel) in image.data.chunks_exact_mut(4).enumerate() {
        let x = (index % width) as f32;
        let y = (index / width) as f32;
        let normalized = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / max_radius;
        let factor = (1.0 - strength * 0.85 * normalized * normalized).clamp(0.05, 3.0);
        pixel[0] *= factor;
        pixel[1] *= factor;
        pixel[2] *= factor;
    }
}

fn grain_noise(x: u32, y: u32) -> f32 {
    let mut hash = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263));
    hash = (hash ^ (hash >> 13)).wrapping_mul(1_274_126_177);
    ((hash ^ (hash >> 16)) as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Deterministic film grain, strongest in the midtones.
pub fn apply_grain(image: &mut LinearImage, amount: f32) {
    let strength = (amount / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 || image.width == 0 {
        return;
    }
    let width = image.width as usize;
    for (index, pixel) in image.data.chunks_exact_mut(4).enumerate() {
        let l = luma(pixel[0], pixel[1], pixel[2]);
        let midtone_weight = (4.0 * l * (1.0 - l)).clamp(0.0, 1.0);
        let noise = grain_noise((index % width) as u32, (index / width) as u32);
        let factor = 1.0 + noise * strength * 0.12 * midtone_weight;
        pixel[0] = (pixel[0] * factor).max(0.0);
        pixel[1] = (pixel[1] * factor).max(0.0);
        pixel[2] = (pixel[2] * factor).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_core::DecodedImage;

    fn gray_image(width: u32, height: u32, value: u8) -> LinearImage {
        let data = vec![value; (width * height * 4) as usize];
        DecodedImage::new(width, height, data).unwrap().to_linear()
    }

    fn center_levels(image: &LinearImage) -> Vec<f32> {
        image
            .data
            .chunks_exact(4)
            .map(|pixel| pixel[0] * 0.2126 + pixel[1] * 0.7152 + pixel[2] * 0.0722)
            .collect()
    }

    #[test]
    fn denoise_reduces_noise_but_keeps_an_edge() {
        let mut noisy = vec![0u8; 32 * 32 * 4];
        for y in 0..32usize {
            for x in 0..32usize {
                let base = if x < 16 { 60u8 } else { 200u8 };
                let noise = if (x + y) % 2 == 0 { 12 } else { 0 };
                let value = base.saturating_add(noise);
                let index = (y * 32 + x) * 4;
                noisy[index] = value;
                noisy[index + 1] = value;
                noisy[index + 2] = value;
                noisy[index + 3] = 255;
            }
        }
        let mut image = DecodedImage::new(32, 32, noisy).unwrap().to_linear();
        let before = center_levels(&image);
        apply_noise_reduction(&mut image, 80.0, 0.0);
        let after = center_levels(&image);

        let variance = |values: &[f32], left: bool| {
            let slice: Vec<f32> = values
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    let x = index % 32;
                    if left {
                        (2..14).contains(&x)
                    } else {
                        (18..30).contains(&x)
                    }
                })
                .map(|(_, value)| *value)
                .collect();
            let mean = slice.iter().sum::<f32>() / slice.len() as f32;
            slice.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / slice.len() as f32
        };
        assert!(variance(&after, true) < variance(&before, true));
        let left = after[10 * 32 + 8];
        let right = after[10 * 32 + 24];
        assert!(
            right > left * 1.8,
            "the edge must survive: {left} vs {right}"
        );
    }

    #[test]
    fn vignette_darkens_only_the_corners() {
        let mut image = gray_image(32, 32, 180);
        apply_vignette(&mut image, 80.0);
        let center = image.data[(16 * 32 + 16) * 4];
        let corner = image.data[0];
        assert!((center - gray_image(1, 1, 180).data[0]).abs() < 1e-3);
        assert!(corner < center * 0.7, "corner {corner} vs centre {center}");
    }

    #[test]
    fn grain_is_deterministic_and_off_at_zero() {
        let original = gray_image(16, 16, 140);
        let mut untouched = original.clone();
        apply_grain(&mut untouched, 0.0);
        assert_eq!(untouched.data, original.data);

        let mut first = original.clone();
        let mut second = original.clone();
        apply_grain(&mut first, 60.0);
        apply_grain(&mut second, 60.0);
        assert_eq!(first.data, second.data);
        assert_ne!(first.data, original.data);
    }

    #[test]
    fn dehaze_and_chromatic_are_noops_at_zero_and_finite_otherwise() {
        let original = gray_image(24, 24, 120);
        let mut untouched = original.clone();
        apply_dehaze(&mut untouched, 0.0);
        apply_chromatic(&mut untouched, 0.0);
        assert_eq!(untouched.data, original.data);

        let mut changed = original.clone();
        apply_dehaze(&mut changed, 50.0);
        apply_chromatic(&mut changed, 50.0);
        assert!(changed.data.iter().all(|value| value.is_finite()));
    }
}
