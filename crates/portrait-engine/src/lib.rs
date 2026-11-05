//! Portrait retouching for the CPU reference path.
//!
//! The base effects (smooth, even skin, brighten, body smoothing) work on a
//! broad warm-chroma skin mask. The fine-repair effects (eyes, teeth, under-eye
//! circles, wrinkles and skin defects) need measured facial geometry, which the
//! document stores on the portrait layer. Without that geometry those effects
//! stay off instead of guessing where the features are.

use document::PortraitParams;
use image_core::{box_blur, linear_to_srgb, srgb_to_linear, DecodedImage, LinearImage};
use rayon::prelude::*;

mod fine;
mod warp;
pub use warp::{apply_body_shape, apply_face_shape};

pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn clamp01(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(crate) fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub(crate) fn rgb_to_ycbcr(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = (b - y) * 0.564 + 0.5;
    let cr = (r - y) * 0.713 + 0.5;
    (y, cb, cr)
}

fn ycbcr_to_rgb(y: f32, cb: f32, cr: f32) -> (f32, f32, f32) {
    let cb = cb - 0.5;
    let cr = cr - 0.5;
    (y + 1.403 * cr, y - 0.344 * cb - 0.714 * cr, y + 1.773 * cb)
}

fn skin_weights(linear: [f32; 4]) -> (f32, f32) {
    let r = clamp01(linear_to_srgb(linear[0]));
    let g = clamp01(linear_to_srgb(linear[1]));
    let b = clamp01(linear_to_srgb(linear[2]));
    let (y, cb, cr) = rgb_to_ycbcr(r, g, b);
    let order = if r >= b { 1.0 } else { 0.3 };
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = if max > 1e-5 { (max - min) / max } else { 0.0 };
    let chroma_weight = 0.4 + 0.6 * chroma.clamp(0.0, 1.0);

    // Warm chroma window (Cb/Cr) + reasonable luminance.
    let face_warm = smoothstep(0.50, 0.57, cr) * (1.0 - smoothstep(0.68, 0.78, cr));
    let face_cb = smoothstep(0.28, 0.36, cb) * (1.0 - smoothstep(0.56, 0.64, cb));
    let face_y = smoothstep(0.06, 0.18, y) * (1.0 - smoothstep(0.88, 0.98, y));
    let skin = face_warm * face_cb * face_y * order * chroma_weight;

    // Wider, weaker window for body-like skin tones. Neutral greys, black
    // clothing and white walls sit near Cb/Cr 0.5, so a chroma gate is
    // required; without it the body mask touches the background.
    let body_warm = smoothstep(0.46, 0.54, cr) * (1.0 - smoothstep(0.74, 0.84, cr));
    let body_cb = smoothstep(0.24, 0.32, cb) * (1.0 - smoothstep(0.60, 0.70, cb));
    let body_y = smoothstep(0.03, 0.12, y) * (1.0 - smoothstep(0.92, 1.0, y));
    let body_chroma = smoothstep(0.04, 0.12, chroma);
    let body = body_warm * body_cb * body_y * order * (0.5 + 0.5 * chroma_weight) * body_chroma;

    (skin, body)
}

fn transpose_mask(data: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; data.len()];
    out.par_chunks_mut(height).enumerate().for_each(|(x, row)| {
        for y in 0..height {
            row[y] = data[y * width + x];
        }
    });
    out
}

fn horizontal_blur_mask(data: &[f32], width: usize, radius: usize) -> Vec<f32> {
    let radius = radius.min(width.saturating_sub(1));
    let mut out = vec![0.0f32; data.len()];
    out.par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row_out)| {
            let row = &data[y * width..(y + 1) * width];
            let mut prefix = vec![0.0f32; width + 1];
            for x in 0..width {
                prefix[x + 1] = prefix[x] + row[x];
            }
            for (x, value) in row_out.iter_mut().enumerate() {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(width - 1);
                *value = (prefix[hi + 1] - prefix[lo]) / (hi - lo + 1) as f32;
            }
        });
    out
}

fn feather_mask(mask: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return mask.to_vec();
    }
    let horizontal = horizontal_blur_mask(mask, width, radius);
    let transposed = transpose_mask(&horizontal, width, height);
    let blurred = horizontal_blur_mask(&transposed, height, radius);
    transpose_mask(&blurred, height, width)
}

/// Per-pixel heuristic skin weights for automatic local masks. This is a
/// color-range heuristic, not semantic face segmentation.
pub fn skin_mask(image: &LinearImage) -> Vec<f32> {
    let mut mask = vec![0.0f32; image.pixel_count()];
    mask.par_iter_mut().enumerate().for_each(|(index, weight)| {
        let base = index * 4;
        let (skin, _) = skin_weights([
            image.data[base],
            image.data[base + 1],
            image.data[base + 2],
            image.data[base + 3],
        ]);
        *weight = skin;
    });
    mask
}

/// Apply model-free portrait adjustments in place. Alpha is never modified.
pub fn apply_portrait_adjustments(image: &mut LinearImage, params: &PortraitParams) {
    let p = params.sanitized();
    if !p.has_active_effect() {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    let pixel_count = width * height;
    if pixel_count == 0 {
        return;
    }
    let shortest = width.min(height) as f32;
    let intensity = p.intensity / 100.0;

    let face_mode = !p.body_mode;
    let needs_body = p.body_mode || p.body_smooth > 0.0;
    let needs_skin = face_mode
        && (p.body_smooth > 0.0
            || p.smooth > 0.0
            || p.even_skin > 0.0
            || p.brighten > 0.0
            || p.whiten > 0.0
            || p.rosy > 0.0
            || p.blemish > 0.0
            || p.freckle > 0.0
            || p.mole > 0.0
            || p.birthmark > 0.0
            || p.tattoo > 0.0
            || p.dark_circle > 0.0
            || p.eye_bag > 0.0);

    let mut skin = Vec::new();
    if needs_skin {
        skin = vec![0.0f32; pixel_count];
        skin.par_iter_mut().enumerate().for_each(|(i, skin)| {
            let base = i * 4;
            let (weight, _) = skin_weights([
                image.data[base],
                image.data[base + 1],
                image.data[base + 2],
                image.data[base + 3],
            ]);
            *skin = weight;
        });
        let mask_radius = ((shortest / 400.0).round() as usize).clamp(2, 12);
        skin = feather_mask(&skin, width, height, mask_radius);
    }
    let body = if needs_body {
        let mut body = vec![0.0f32; pixel_count];
        body.par_iter_mut().enumerate().for_each(|(i, weight)| {
            let base = i * 4;
            let (_, candidate) = skin_weights([
                image.data[base],
                image.data[base + 1],
                image.data[base + 2],
                image.data[base + 3],
            ]);
            *weight = candidate;
        });
        feather_mask(
            &body,
            width,
            height,
            ((shortest / 250.0).round() as usize).clamp(3, 16),
        )
    } else {
        Vec::new()
    };

    if face_mode && p.smooth > 0.0 {
        let base_radius = ((shortest / 300.0).round() as u32).clamp(3, 14);
        let radius = ((base_radius as f32) * (1.0 + p.smooth_radius / 100.0 * 1.2))
            .round()
            .clamp(2.0, 32.0) as u32;
        let low = box_blur(image, radius);
        let high_scale = 0.15 + 0.85 * (p.texture / 100.0);
        let strength = p.smooth / 100.0 * 0.85 * intensity;
        image
            .data
            .par_chunks_mut(4)
            .zip(low.data.par_chunks(4))
            .enumerate()
            .for_each(|(i, (px, low))| {
                let amount = strength * skin[i];
                if amount <= 1e-6 {
                    return;
                }
                for c in 0..3 {
                    let high = px[c] - low[c];
                    let target = low[c] + high * high_scale;
                    px[c] = clamp01(px[c] + (target - px[c]) * amount);
                }
            });
    }

    let mut mean_luma = 0.5f32;
    let mut mean_cr = 0.5f32;
    let mut mean_cb = 0.5f32;
    let mut has_mean = false;
    if face_mode
        && needs_skin
        && (p.even_skin > 0.0 || p.dark_circle > 0.0 || p.eye_bag > 0.0)
    {
        let mut weight_sum = 0.0f32;
        let mut cr_sum = 0.0f32;
        let mut cb_sum = 0.0f32;
        let mut luma_sum = 0.0f32;
        for (i, &mask) in skin.iter().take(pixel_count).enumerate() {
            if mask <= 0.01 {
                continue;
            }
            let base = i * 4;
            let r = clamp01(linear_to_srgb(image.data[base]));
            let g = clamp01(linear_to_srgb(image.data[base + 1]));
            let b = clamp01(linear_to_srgb(image.data[base + 2]));
            let (value, cb, cr) = rgb_to_ycbcr(r, g, b);
            let weight = mask * mask;
            weight_sum += weight;
            cr_sum += cr * weight;
            cb_sum += cb * weight;
            luma_sum += value * weight;
        }
        if weight_sum > 1e-6 {
            mean_cr = cr_sum / weight_sum;
            mean_cb = cb_sum / weight_sum;
            mean_luma = luma_sum / weight_sum;
            has_mean = true;
        }
    }
    let even_skin = if has_mean { p.even_skin } else { 0.0 };

    if face_mode && (even_skin > 0.0 || p.brighten > 0.0) {
        image
            .data
            .par_chunks_mut(4)
            .enumerate()
            .for_each(|(i, px)| {
                let mask = skin[i];
                if mask <= 1e-4 {
                    return;
                }
                let mut r = clamp01(linear_to_srgb(px[0]));
                let mut g = clamp01(linear_to_srgb(px[1]));
                let mut b = clamp01(linear_to_srgb(px[2]));

                if even_skin > 0.0 {
                    let (y, cb, cr) = rgb_to_ycbcr(r, g, b);
                    let amount = even_skin / 100.0 * 0.45 * intensity * mask;
                    let cb = cb + (mean_cb - cb) * amount;
                    let cr = cr + (mean_cr - cr) * amount;
                    let (nr, ng, nb) = ycbcr_to_rgb(y, cb, cr);
                    r = clamp01(nr);
                    g = clamp01(ng);
                    b = clamp01(nb);
                }

                let mut lr = srgb_to_linear(r);
                let mut lg = srgb_to_linear(g);
                let mut lb = srgb_to_linear(b);
                if p.brighten > 0.0 {
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let rolloff = (1.0 - luma).clamp(0.0, 1.0);
                    let gain =
                        1.0 + p.brighten / 100.0 * 0.30 * intensity * mask * rolloff * rolloff;
                    lr *= gain;
                    lg *= gain;
                    lb *= gain;
                }
                px[0] = clamp01(lr);
                px[1] = clamp01(lg);
                px[2] = clamp01(lb);
                // Alpha untouched.
            });
    }

    let skin_mean = if has_mean && (p.dark_circle > 0.0 || p.eye_bag > 0.0) {
        Some((mean_luma, mean_cb, mean_cr))
    } else {
        None
    };
    let masks = fine::FineMasks {
        face: if needs_skin { Some(skin.as_slice()) } else { None },
        body: if needs_body {
            Some(body.as_slice())
        } else {
            None
        },
        mean: skin_mean,
        body_mode: p.body_mode,
    };
    fine::apply_fine_repair(image, &p, &masks, intensity);
}

pub mod analysis;
mod onnx;
pub mod regions;

pub use analysis::{
    face_model_available, EnsembleFaceAnalyzer, FaceAnalyzer, FaceBox, PortraitAnalysis,
    PortraitWarning, UnavailableFaceAnalyzer, FACE_MODEL_NAME,
};

/// Small helper for callers that want to report why no analysis exists without
/// constructing an analyzer.
pub fn unavailable_analysis() -> PortraitAnalysis {
    UnavailableFaceAnalyzer.analyze(&DecodedImage {
        width: 0,
        height: 0,
        rgba8: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::PortraitParams;

    fn skin_patch(width: u32, height: u32) -> LinearImage {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let delta: f32 = if (x + y) % 2 == 0 { 10.0 } else { -10.0 };
                let r = (210.0 + delta).clamp(0.0, 255.0) as u8;
                let g = (165.0 + delta).clamp(0.0, 255.0) as u8;
                let b = (140.0 + delta * 0.5).clamp(0.0, 255.0) as u8;
                rgba.extend_from_slice(&[r, g, b, 200]);
            }
        }
        DecodedImage::new(width, height, rgba).unwrap().to_linear()
    }

    fn luma_variance(image: &LinearImage, x0: u32, y0: u32, size: u32) -> f32 {
        let mut values = Vec::new();
        for y in y0..y0 + size {
            for x in x0..x0 + size {
                let p = image.pixel(x, y).unwrap();
                values.push(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]);
            }
        }
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / values.len() as f32
    }

    #[test]
    fn zero_params_are_bitwise_noop() {
        let mut image = skin_patch(48, 48);
        let before = image.data.clone();
        apply_portrait_adjustments(&mut image, &PortraitParams::default());
        assert_eq!(image.data, before);

        let zero = PortraitParams {
            intensity: 0.0,
            smooth: 80.0,
            even_skin: 50.0,
            ..Default::default()
        };
        apply_portrait_adjustments(&mut image, &zero);
        assert_eq!(image.data, before);
    }

    #[test]
    fn smoothing_reduces_skin_local_variance() {
        let mut image = skin_patch(200, 200);
        let before = luma_variance(&image, 40, 40, 120);
        let params = PortraitParams {
            smooth: 100.0,
            texture: 0.0,
            ..Default::default()
        };
        apply_portrait_adjustments(&mut image, &params);
        let after = luma_variance(&image, 40, 40, 120);
        assert!(
            after < before * 0.8,
            "variance should drop: before={before}, after={after}"
        );
    }

    #[test]
    fn alpha_is_preserved() {
        let mut image = skin_patch(64, 64);
        let alphas: Vec<f32> = image.data.chunks_exact(4).map(|p| p[3]).collect();
        apply_portrait_adjustments(
            &mut image,
            &PortraitParams {
                smooth: 100.0,
                even_skin: 100.0,
                brighten: 100.0,
                body_smooth: 100.0,
                ..Default::default()
            },
        );
        let after: Vec<f32> = image.data.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(alphas, after);
    }

    #[test]
    fn unavailable_analyzer_reports_chinese_warning() {
        let analysis = unavailable_analysis();
        assert!(!analysis.available);
        assert_eq!(analysis.warnings[0].code, "model_not_configured");
        assert!(analysis.warnings[0].message.contains("人像识别"));
    }
}
