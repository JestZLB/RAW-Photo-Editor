//! CPU reference color pipeline.
//!
//! All processing happens in linear sRGB. Contrast and tone controls are
//! evaluated in a display-referred (sRGB encoded) space around a 0.5 pivot,
//! which keeps the response familiar while the storage format stays linear.

use document::ColorParams;
use image_core::{box_blur, linear_to_srgb, srgb_to_linear, LinearImage};
use rayon::prelude::*;

mod effects;
mod lut;
pub use effects::{
    apply_chromatic, apply_dehaze, apply_grain, apply_noise_reduction, apply_vignette,
};
pub use lut::{apply_lut, parse_cube, Lut};

/// Solve the temperature/tint pair that turns a sampled display-referred color
/// into a neutral gray. The gains mirror [`apply_color_adjustments`] exactly,
/// so applying the returned values makes the sampled pixel neutral.
///
/// Returns `None` for black, clipped or non-finite samples, where a correction
/// would be driven by noise or missing channel headroom.
pub fn neutral_white_balance(srgb: [f32; 3]) -> Option<(f32, f32)> {
    if !srgb.iter().all(|value| value.is_finite()) {
        return None;
    }
    let display = srgb.map(|value| value.clamp(0.0, 1.0));
    if display.iter().any(|value| *value >= 0.995) {
        return None;
    }

    let [r, g, b] = display.map(srgb_to_linear);
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if luma < 0.01 || r < 1e-4 || b < 1e-4 {
        return None;
    }

    // Gain_r / gain_b = (1 + 0.25t) / (1 - 0.25t), solve for t.
    let ratio = r / b;
    let temperature = (4.0 * (1.0 - ratio) / (1.0 + ratio) * 100.0).clamp(-100.0, 100.0);

    // Gain_g = 1 - 0.15u, gain_r = gain_b = (1 + 0.25t)(1 + 0.05u).
    let r_balanced = r * (1.0 + 0.25 * temperature / 100.0);
    let denominator = 0.15 * g + 0.05 * r_balanced;
    let tint = if denominator.abs() < 1e-6 {
        0.0
    } else {
        ((g - r_balanced) / denominator * 100.0).clamp(-100.0, 100.0)
    };

    Some((round_tenth(temperature), round_tenth(tint)))
}

fn round_tenth(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// Conservative automatic tone suggestion derived from a display-referred
/// luminance histogram. Percentiles are stretched with bounded adjustments so
/// auto correction never turns into an aggressive filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoToneSuggestion {
    pub exposure: f32,
    pub contrast: f32,
    pub whites: f32,
    pub blacks: f32,
}

pub fn auto_tone_from_luma_histogram(histogram: &[u32; 256]) -> Option<AutoToneSuggestion> {
    let total: u64 = histogram.iter().map(|value| *value as u64).sum();
    if total == 0 {
        return None;
    }
    let percentile = |quantile: f64| -> f32 {
        let target = ((total as f64 * quantile).ceil() as u64).max(1);
        let mut cumulative = 0u64;
        for (index, count) in histogram.iter().enumerate() {
            cumulative += *count as u64;
            if cumulative >= target {
                return index as f32 / 255.0;
            }
        }
        1.0
    };

    let shadows = percentile(0.02);
    let median = percentile(0.5).max(0.04);
    let highlights = percentile(0.98);
    let median_exposure = (0.42f32 / median).log2();
    let highlight_room = (0.96f32 / highlights.max(0.05)).log2().max(0.0);
    // Never brighten into already-bright highlights; darkening remains allowed.
    let exposure = if median_exposure > 0.0 {
        median_exposure.min(highlight_room)
    } else {
        median_exposure
    }
    .clamp(-1.0, 1.0);
    let spread = highlights - shadows;
    let contrast = if spread < 0.42 {
        ((0.42 - spread) / 0.42 * 15.0).clamp(0.0, 15.0)
    } else if spread > 0.9 {
        ((0.9 - spread) / 0.4 * 10.0).clamp(-10.0, 0.0)
    } else {
        0.0
    };
    let blacks = ((0.04 - shadows) * 80.0).clamp(-8.0, 4.0);
    let whites = ((0.94 - highlights) * 60.0).clamp(-8.0, 4.0);

    Some(AutoToneSuggestion {
        exposure: (exposure * 100.0).round() / 100.0,
        contrast: round_tenth(contrast),
        whites: round_tenth(whites),
        blacks: round_tenth(blacks),
    })
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) * 0.5;
    let delta = max - min;
    if delta <= 1e-6 {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(1e-6);
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    (hue, saturation.clamp(0.0, 1.0), lightness)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> (f32, f32, f32) {
    let hue = hue.rem_euclid(1.0);
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue * 6.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue * 6.0) as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma * 0.5;
    (
        (r + m).clamp(0.0, 1.0),
        (g + m).clamp(0.0, 1.0),
        (b + m).clamp(0.0, 1.0),
    )
}

fn sample_curve(points: [f32; 5], values: [f32; 5], input: f32) -> f32 {
    let x = input.clamp(0.0, 1.0);
    for index in 0..4 {
        if x <= points[index + 1] {
            let span = (points[index + 1] - points[index]).max(1e-6);
            let t = ((x - points[index]) / span).clamp(0.0, 1.0);
            return values[index] + (values[index + 1] - values[index]) * t;
        }
    }
    values[4]
}

fn build_tone_lut(params: &ColorParams) -> [f32; 256] {
    let mut lut = [0.0f32; 256];
    let input_range = (params.levels_in_white - params.levels_in_black).max(1e-6);
    let output_range = params.levels_out_white - params.levels_out_black;
    for (index, value) in lut.iter_mut().enumerate() {
        let normalized = (index as f32 / 255.0 - params.levels_in_black) / input_range;
        let normalized = normalized.clamp(0.0, 1.0).powf(params.levels_gamma);
        let leveled = params.levels_out_black + normalized * output_range;
        *value = sample_curve(params.curve_x, params.curve_y, leveled).clamp(0.0, 1.0);
    }
    lut
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn apply_color_adjustments(image: &mut LinearImage, params: &ColorParams) {
    let p = params.sanitized();
    if p.without_lut() == ColorParams::default() {
        // Bitwise no-op for neutral parameters; the LUT is applied by the
        // renderer, not here.
        return;
    }

    apply_chromatic(image, p.chromatic);
    apply_noise_reduction(image, p.noise_luma, p.noise_chroma);
    apply_dehaze(image, p.dehaze);

    let exposure_gain = 2.0f32.powf(p.exposure);
    let temperature = p.temperature / 100.0;
    let tint = p.tint / 100.0;
    let gain_r = (1.0 + 0.25 * temperature) * (1.0 + 0.05 * tint);
    let gain_g = 1.0 - 0.15 * tint;
    let gain_b = (1.0 - 0.25 * temperature) * (1.0 + 0.05 * tint);
    let contrast = 1.0 + p.contrast / 100.0;
    let shadows = p.shadows / 100.0 * 0.35;
    let highlights = p.highlights / 100.0 * 0.35;
    let blacks = p.blacks / 100.0 * 0.25;
    let whites = p.whites / 100.0 * 0.25;
    let saturation = 1.0 + p.saturation / 100.0;
    let vibrance = p.vibrance / 100.0;
    let hue_shift = p.hue / 360.0;
    let hsl_saturation = 1.0 + p.hsl_saturation / 100.0;
    let hsl_lightness = p.hsl_lightness / 100.0 * 0.35;
    let hsl_active = p.hue != 0.0 || p.hsl_saturation != 0.0 || p.hsl_lightness != 0.0;
    let tone_active = p.uses_tone_curve();
    let tone_lut = if tone_active {
        Some(build_tone_lut(&p))
    } else {
        None
    };

    image.data.par_chunks_mut(4).for_each(|px| {
        let mut r = px[0] * gain_r * exposure_gain;
        let mut g = px[1] * gain_g * exposure_gain;
        let mut b = px[2] * gain_b * exposure_gain;

        // Display-referred tone work: sRGB encode, contrast around 0.5, tone
        // masks, then decode back to linear.
        let mut dr = linear_to_srgb(r).clamp(0.0, 1.0);
        let mut dg = linear_to_srgb(g).clamp(0.0, 1.0);
        let mut db = linear_to_srgb(b).clamp(0.0, 1.0);

        dr = (dr - 0.5) * contrast + 0.5;
        dg = (dg - 0.5) * contrast + 0.5;
        db = (db - 0.5) * contrast + 0.5;

        let luma = 0.2126 * dr + 0.7152 * dg + 0.0722 * db;
        let shadow_mask = 1.0 - smoothstep(0.0, 0.65, luma);
        let highlight_mask = smoothstep(0.35, 1.0, luma);
        let black_mask = 1.0 - smoothstep(0.0, 0.4, luma);
        let white_mask = smoothstep(0.6, 1.0, luma);
        let offset = shadows * shadow_mask
            + highlights * highlight_mask
            + blacks * black_mask
            + whites * white_mask;
        dr += offset;
        dg += offset;
        db += offset;

        dr = dr.clamp(0.0, 1.0);
        dg = dg.clamp(0.0, 1.0);
        db = db.clamp(0.0, 1.0);

        if hsl_active {
            let (mut hue, value, lightness) = rgb_to_hsl(dr, dg, db);
            hue = (hue + hue_shift).rem_euclid(1.0);
            let saturation = (value * hsl_saturation).clamp(0.0, 1.0);
            let lightness = if hsl_lightness >= 0.0 {
                lightness + (1.0 - lightness) * hsl_lightness
            } else {
                lightness * (1.0 + hsl_lightness)
            };
            let (next_r, next_g, next_b) = hsl_to_rgb(hue, saturation, lightness.clamp(0.0, 1.0));
            dr = next_r;
            dg = next_g;
            db = next_b;
        }

        if let Some(lut) = &tone_lut {
            let lookup = |value: f32| {
                let index = (value.clamp(0.0, 1.0) * 255.0).round() as usize;
                lut[index.min(255)]
            };
            dr = lookup(dr);
            dg = lookup(dg);
            db = lookup(db);
        }

        r = srgb_to_linear(dr);
        g = srgb_to_linear(dg);
        b = srgb_to_linear(db);

        let luma_linear = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = if max > 1e-6 {
            ((max - min) / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let vibrance_gain = 1.0 + vibrance * (1.0 - chroma) * 0.8;
        let factor = (saturation * vibrance_gain).max(0.0);
        r = luma_linear + (r - luma_linear) * factor;
        g = luma_linear + (g - luma_linear) * factor;
        b = luma_linear + (b - luma_linear) * factor;

        px[0] = sanitize(r);
        px[1] = sanitize(g);
        px[2] = sanitize(b);
        // Alpha is intentionally untouched.
    });

    if p.clarity != 0.0 {
        let shortest = image.width.min(image.height) as f32;
        let radius = ((shortest / 20.0).round() as u32).clamp(3, 64);
        let blurred = box_blur(image, radius);
        let amount = p.clarity / 100.0 * 0.6;
        image
            .data
            .par_chunks_mut(4)
            .zip(blurred.data.par_chunks(4))
            .for_each(|(px, blurred)| {
                for c in 0..3 {
                    px[c] = sanitize(px[c] + (px[c] - blurred[c]) * amount);
                }
            });
    }

    if p.sharpness != 0.0 {
        let shortest = image.width.min(image.height) as f32;
        let radius = ((shortest / 1000.0).round() as u32).clamp(1, 3);
        let blurred = box_blur(image, radius);
        let amount = p.sharpness / 100.0 * 0.8;
        image
            .data
            .par_chunks_mut(4)
            .zip(blurred.data.par_chunks(4))
            .for_each(|(px, blurred)| {
                for c in 0..3 {
                    px[c] = sanitize(px[c] + (px[c] - blurred[c]) * amount);
                }
            });
    }

    apply_vignette(image, p.vignette);
    apply_grain(image, p.grain);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_core::DecodedImage;

    fn test_image() -> LinearImage {
        let mut rgba = Vec::new();
        for y in 0..32u32 {
            for x in 0..32u32 {
                rgba.extend_from_slice(&[
                    (40 + x * 3) as u8,
                    (30 + y * 3) as u8,
                    (20 + (x + y)) as u8,
                    255,
                ]);
            }
        }
        DecodedImage::new(32, 32, rgba).unwrap().to_linear()
    }

    fn mean_luminance(image: &LinearImage) -> f32 {
        image
            .data
            .chunks_exact(4)
            .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
            .sum::<f32>()
            / image.pixel_count() as f32
    }

    #[test]
    fn zero_params_are_bitwise_noop() {
        let mut image = test_image();
        let before = image.data.clone();
        apply_color_adjustments(&mut image, &ColorParams::default());
        assert_eq!(image.data, before);
    }

    #[test]
    fn positive_exposure_increases_mean_luminance() {
        let mut image = test_image();
        let before = mean_luminance(&image);
        apply_color_adjustments(
            &mut image,
            &ColorParams {
                exposure: 1.0,
                ..Default::default()
            },
        );
        assert!(mean_luminance(&image) > before * 1.5);
    }

    #[test]
    fn full_desaturation_is_near_gray() {
        let mut image = test_image();
        apply_color_adjustments(
            &mut image,
            &ColorParams {
                saturation: -100.0,
                ..Default::default()
            },
        );
        for pixel in image.data.chunks_exact(4) {
            assert!((pixel[0] - pixel[1]).abs() < 1e-4);
            assert!((pixel[1] - pixel[2]).abs() < 1e-4);
        }
    }

    #[test]
    fn curve_and_levels_adjust_display_tone() {
        let mut curve = [0.0f32; 5];
        for (index, value) in curve.iter_mut().enumerate() {
            *value = index as f32 / 4.0;
        }
        curve[2] = 0.8;
        let mut image = solid_linear([0.5, 0.5, 0.5]);
        apply_color_adjustments(
            &mut image,
            &ColorParams {
                curve_y: curve,
                ..Default::default()
            },
        );
        assert!(image.data[0] > 0.5);

        let mut levels = solid_linear([0.15, 0.15, 0.15]);
        apply_color_adjustments(
            &mut levels,
            &ColorParams {
                levels_in_black: 0.2,
                ..Default::default()
            },
        );
        assert!(levels.data[0] < 1e-5);
    }

    #[test]
    fn hsl_hue_rotates_red_and_zero_saturation_is_gray() {
        let mut rotated = solid_linear([0.8, 0.1, 0.1]);
        apply_color_adjustments(
            &mut rotated,
            &ColorParams {
                hue: 180.0,
                ..Default::default()
            },
        );
        let pixel = &rotated.data[..3];
        assert!(pixel[0] < pixel[1] && pixel[0] < pixel[2]);

        let mut gray = solid_linear([0.8, 0.2, 0.1]);
        apply_color_adjustments(
            &mut gray,
            &ColorParams {
                hsl_saturation: -100.0,
                ..Default::default()
            },
        );
        let pixel = &gray.data[..3];
        assert!((pixel[0] - pixel[1]).abs() < 1e-3);
        assert!((pixel[1] - pixel[2]).abs() < 1e-3);
    }

    #[test]
    fn non_finite_params_are_sanitized() {
        let mut image = test_image();
        let params = ColorParams {
            exposure: f32::NAN,
            contrast: f32::INFINITY,
            saturation: -f32::INFINITY,
            ..Default::default()
        };
        apply_color_adjustments(&mut image, &params);
        assert!(image.data.iter().all(|v| v.is_finite()));
    }

    fn solid_linear(display: [f32; 3]) -> LinearImage {
        let mut rgba = display
            .iter()
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect::<Vec<_>>();
        rgba.push(255);
        DecodedImage::new(1, 1, rgba).unwrap().to_linear()
    }

    #[test]
    fn white_balance_neutralizes_warm_and_cool_gray_samples() {
        for bytes in [[180u8, 170, 160], [140, 150, 165], [170, 166, 160]] {
            let sample = bytes.map(|value| value as f32 / 255.0);
            let (temperature, tint) =
                neutral_white_balance(sample).expect("mid-tone sample should be usable");
            assert!((-100.0..=100.0).contains(&temperature));
            assert!((-100.0..=100.0).contains(&tint));

            let mut image = solid_linear(sample);
            apply_color_adjustments(
                &mut image,
                &ColorParams {
                    temperature,
                    tint,
                    ..Default::default()
                },
            );
            let pixel = &image.data[..3];
            let max = pixel[0].max(pixel[1]).max(pixel[2]);
            let min = pixel[0].min(pixel[1]).min(pixel[2]);
            assert!(
                max - min < 4e-3,
                "sample {bytes:?} did not neutralize: {pixel:?} with t={temperature}, u={tint}"
            );
        }
    }

    #[test]
    fn auto_tone_brightens_dark_and_tones_down_bright_images() {
        let mut dark = [0u32; 256];
        dark[51] = 10_000;
        let dark_suggestion = auto_tone_from_luma_histogram(&dark).unwrap();
        assert!(dark_suggestion.exposure > 0.8);
        assert!((0.0..=15.0).contains(&dark_suggestion.contrast));

        let mut bright = [0u32; 256];
        bright[255] = 10_000;
        let bright_suggestion = auto_tone_from_luma_histogram(&bright).unwrap();
        assert!(bright_suggestion.exposure < -0.5);
        assert!(bright_suggestion.whites < 0.0);
    }

    #[test]
    fn auto_tone_is_bounded_and_rejects_empty_histograms() {
        assert!(auto_tone_from_luma_histogram(&[0; 256]).is_none());
        let mut histogram = [0u32; 256];
        for (index, count) in histogram.iter_mut().enumerate() {
            *count = if index == 0 || index == 255 {
                2000
            } else {
                100
            };
        }
        let suggestion = auto_tone_from_luma_histogram(&histogram).unwrap();
        assert!((-1.0..=1.0).contains(&suggestion.exposure));
        assert!((-10.0..=15.0).contains(&suggestion.contrast));
        assert!((-8.0..=4.0).contains(&suggestion.whites));
        assert!((-8.0..=4.0).contains(&suggestion.blacks));
    }

    #[test]
    fn white_balance_rejects_black_clipped_and_non_finite_samples() {
        assert!(neutral_white_balance([0.0, 0.0, 0.0]).is_none());
        assert!(neutral_white_balance([0.01, 0.01, 0.015]).is_none());
        assert!(neutral_white_balance([1.0, 1.0, 1.0]).is_none());
        assert!(neutral_white_balance([0.7, f32::NAN, 0.6]).is_none());
    }

    #[test]
    fn alpha_is_preserved() {
        let mut image = test_image();
        for (i, pixel) in image.data.chunks_exact_mut(4).enumerate() {
            pixel[3] = (i % 256) as f32 / 255.0;
        }
        let alphas: Vec<f32> = image.data.chunks_exact(4).map(|p| p[3]).collect();
        apply_color_adjustments(
            &mut image,
            &ColorParams {
                exposure: 0.5,
                saturation: 25.0,
                clarity: 40.0,
                sharpness: 50.0,
                ..Default::default()
            },
        );
        let after: Vec<f32> = image.data.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(alphas, after);
    }
}
