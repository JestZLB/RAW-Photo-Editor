//! Local geometric shaping: face slimming, eye enlargement and nose slimming.
//!
//! The warp is inverse-mapped: every destination pixel samples a source
//! position from a snapshot, so repeated previews are deterministic and the
//! displacement never feeds back into itself. All weights taper to zero well
//! before the face mask edge, which keeps the composite seam free.

use document::{PortraitEllipse, PortraitParams};
use image_core::LinearImage;

use crate::smoothstep;

struct Ellipse {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
}

impl Ellipse {
    fn pixels(shape: &PortraitEllipse, width: u32, height: u32) -> Option<Self> {
        let rx = shape.radius_x * width as f32;
        let ry = shape.radius_y * height as f32;
        if rx < 1.0 || ry < 1.0 {
            return None;
        }
        Some(Self {
            cx: shape.center_x * width as f32,
            cy: shape.center_y * height as f32,
            rx,
            ry,
        })
    }
}

/// Apply face shape controls in place. Callers must skip hidden/zero-opacity
/// layers before invoking this.
pub fn apply_face_shape(image: &mut LinearImage, params: &PortraitParams) {
    let p = params.sanitized();
    if !p.has_shape_effect() {
        return;
    }
    let Some(geometry) = p.geometry else {
        return;
    };
    let Some(face) = geometry
        .face
        .and_then(|shape| Ellipse::pixels(&shape, image.width, image.height))
    else {
        return;
    };
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 {
        return;
    }
    let snapshot = image.clone();
    let left = geometry
        .left_eye
        .and_then(|shape| normalized_eye(shape, &face, image.width, image.height))
        .unwrap_or_else(|| derived_eye(&face, true));
    let right = geometry
        .right_eye
        .and_then(|shape| normalized_eye(shape, &face, image.width, image.height))
        .unwrap_or_else(|| derived_eye(&face, false));
    let eye_y = (left.cy + right.cy) * 0.5;
    let mouth_y = geometry
        .mouth
        .map(|shape| shape.center_y * image.height as f32)
        .unwrap_or(eye_y + face.ry * 0.62);
    let nose = Ellipse {
        cx: face.cx,
        cy: eye_y + (mouth_y - eye_y) * 0.55,
        rx: (face.rx * 0.22).max(2.0),
        ry: (face.ry * 0.28).max(2.0),
    };
    let slim = p.face_slim / 100.0;
    let eye_size = p.eye_size / 100.0;
    let nose_slim = p.nose_slim / 100.0;
    let jaw = p.jaw_width / 100.0;
    let chin = p.chin_length / 100.0;
    let eye_gap = p.eye_distance / 100.0;

    let x0 = ((face.cx - face.rx * 1.6).floor().max(0.0)) as usize;
    let x1 = ((face.cx + face.rx * 1.6).ceil().min(width as f32 - 1.0)) as usize;
    let y0 = ((face.cy - face.ry * 1.5).floor().max(0.0)) as usize;
    let y1 = ((face.cy + face.ry * 1.5).ceil().min(height as f32 - 1.0)) as usize;
    if x0 > x1 || y0 > y1 {
        return;
    }
    for y in y0..=y1 {
        for x in x0..=x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let mut sx = px;
            let mut sy = py;

            if slim > 0.0 {
                let dx = (px - face.cx) / face.rx;
                let dy = (py - face.cy) / face.ry;
                if dx.abs() < 1.5 && dy.abs() < 1.4 {
                    let horizontal = 1.0 - smoothstep(0.45, 1.45, dx.abs());
                    let vertical = smoothstep(-0.45, 0.2, dy)
                        * (1.0 - smoothstep(0.9, 1.35, dy));
                    let k = slim * 0.18 * horizontal * vertical;
                    sx += (px - face.cx) * k;
                }
            }

            if jaw > 0.0 {
                let dx = (px - face.cx) / face.rx;
                let dy = (py - face.cy) / face.ry;
                if dy > 0.05 && dy < 1.2 && dx.abs() < 1.4 {
                    let vertical = smoothstep(0.05, 0.45, dy)
                        * (1.0 - smoothstep(0.95, 1.25, dy));
                    let horizontal = 1.0 - smoothstep(0.45, 1.4, dx.abs());
                    let k = jaw * 0.14 * vertical * horizontal;
                    sx += (px - face.cx) * k;
                }
            }

            if chin.abs() > 0.0 {
                let dx = ((px - face.cx) / face.rx).abs();
                let dy = (py - face.cy) / face.ry;
                if dy > 0.15 && dy < 1.5 && dx < 0.9 {
                    let vertical = smoothstep(0.15, 0.6, dy)
                        * (1.0 - smoothstep(1.15, 1.5, dy));
                    let horizontal = 1.0 - smoothstep(0.25, 0.9, dx);
                    let k = chin * 0.12 * vertical * horizontal;
                    sy -= (py - face.cy) * k;
                }
            }

            if eye_gap.abs() > 0.0 {
                for eye in [&left, &right] {
                    let dx = (sx - eye.cx) / (eye.rx * 3.2).max(2.0);
                    let dy = (sy - eye.cy) / (eye.ry * 2.8).max(2.0);
                    let distance = (dx * dx + dy * dy).sqrt();
                    if distance < 1.0 {
                        let side = if eye.cx < face.cx { -1.0 } else { 1.0 };
                        let shift = eye_gap
                            * face.rx
                            * 0.08
                            * (1.0 - smoothstep(0.0, 1.0, distance));
                        sx -= side * shift;
                    }
                }
            }

            if nose_slim > 0.0 {
                let dx = (px - nose.cx) / nose.rx;
                let dy = (py - nose.cy) / nose.ry;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance < 1.0 {
                    let k = nose_slim * 0.25 * (1.0 - smoothstep(0.0, 1.0, distance));
                    sx += (px - nose.cx) * k;
                }
            }

            if eye_size > 0.0 {
                for eye in [&left, &right] {
                    let dx = (sx - eye.cx) / (eye.rx * 2.2).max(2.0);
                    let dy = (sy - eye.cy) / (eye.ry * 2.4).max(2.0);
                    let distance = (dx * dx + dy * dy).sqrt();
                    if distance < 1.0 {
                        let k = eye_size * 0.35 * (1.0 - smoothstep(0.0, 1.0, distance));
                        sx = eye.cx + (sx - eye.cx) * (1.0 - k);
                        sy = eye.cy + (sy - eye.cy) * (1.0 - k);
                    }
                }
            }

            if (sx - px).abs() < 1e-4 && (sy - py).abs() < 1e-4 {
                continue;
            }
            let index = (y * width + x) * 4;
            let sampled = sample_bilinear(&snapshot, sx, sy);
            image.data[index..index + 4].copy_from_slice(&sampled);
        }
    }
}

/// Reshape the masked body: horizontal narrowing around the coverage centre
/// and a vertical stretch of its lower half (legs).
///
/// `coverage` is the rasterized layer mask at the image size. The mask is the
/// user's explicit confirmation of the reshape area, so unlike face shaping
/// this warp is confined to it; the measured face ellipse is protected so a
/// torso mask that reaches the chin cannot squeeze the face.
/// Returns false when there is nothing to do.
pub fn apply_body_shape(
    image: &mut LinearImage,
    params: &PortraitParams,
    coverage: &[f32],
) -> bool {
    let p = params.sanitized();
    let slim_strength = (p.body_slim / 100.0).clamp(0.0, 1.0);
    let lengthen_strength = (p.body_lengthen / 100.0).clamp(0.0, 1.0);
    let width = image.width as usize;
    let height = image.height as usize;
    if (slim_strength <= 0.0 && lengthen_strength <= 0.0)
        || width == 0
        || height == 0
        || coverage.len() != width * height
    {
        return false;
    }

    let mut total = 0.0f64;
    let mut weighted_x = 0.0f64;
    let mut weighted_y = 0.0f64;
    let mut left = width;
    let mut right = 0usize;
    let mut top = height;
    let mut bottom = 0usize;
    for y in 0..height {
        for x in 0..width {
            let weight = coverage[y * width + x];
            if weight <= 0.01 {
                continue;
            }
            let weight = weight as f64;
            total += weight;
            weighted_x += weight * (x as f64 + 0.5);
            weighted_y += weight * (y as f64 + 0.5);
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    if total < 1.0 || left > right || top > bottom {
        return false;
    }
    let center_x = (weighted_x / total) as f32;
    let center_y = (weighted_y / total) as f32;
    let face = p
        .geometry
        .and_then(|geometry| geometry.face)
        .and_then(|shape| Ellipse::pixels(&shape, image.width, image.height));

    let snapshot = image.clone();
    // Capping both directions keeps extreme masks from tearing the frame while
    // still allowing clearly visible changes at 100%.
    let max_amount = 0.42f32;
    let max_lengthen = 0.35f32;
    for y in top..=bottom {
        for x in left..=right {
            let index = y * width + x;
            let mut weight = coverage[index];
            if weight <= 0.002 {
                continue;
            }
            if let Some(face) = &face {
                let dx = (x as f32 + 0.5 - face.cx) / (face.rx * 1.15).max(1.0);
                let dy = (y as f32 + 0.5 - face.cy) / (face.ry * 1.15).max(1.0);
                let distance = (dx * dx + dy * dy).sqrt();
                weight *= smoothstep(0.9, 1.25, distance);
                if weight <= 0.002 {
                    continue;
                }
            }
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let mut sx = px;
            let mut sy = py;
            if slim_strength > 0.0 {
                let amount = (slim_strength * weight).min(max_amount);
                let factor = 1.0 - amount;
                if factor > 0.5 {
                    sx = center_x + (px - center_x) / factor;
                }
            }
            if lengthen_strength > 0.0 && py > center_y {
                let amount = (lengthen_strength * weight).min(max_lengthen);
                sy = center_y + (py - center_y) / (1.0 + amount);
            }
            if (sx - px).abs() < 0.05 && (sy - py).abs() < 0.05 {
                continue;
            }
            let base = index * 4;
            let sampled = sample_bilinear(&snapshot, sx, sy);
            image.data[base..base + 4].copy_from_slice(&sampled);
        }
    }
    true
}

fn normalized_eye(
    shape: PortraitEllipse,
    face: &Ellipse,
    width: u32,
    height: u32,
) -> Option<Ellipse> {
    let rx = shape.radius_x * width as f32;
    let ry = shape.radius_y * height as f32;
    if rx < 1.0 || ry < 1.0 {
        return None;
    }
    let mut eye = Ellipse {
        cx: shape.center_x * width as f32,
        cy: shape.center_y * height as f32,
        rx,
        ry,
    };
    // Measured eye ellipses are tight around the iris. Grow the warp region
    // to cover the socket, otherwise only the iris moves.
    eye.rx = eye.rx.max(face.rx * 0.20);
    eye.ry = eye.ry.max(face.ry * 0.10);
    Some(eye)
}

fn derived_eye(face: &Ellipse, left: bool) -> Ellipse {
    let side = if left { -1.0 } else { 1.0 };
    Ellipse {
        cx: face.cx + side * face.rx * 0.32,
        cy: face.cy - face.ry * 0.18,
        rx: (face.rx * 0.20).max(2.0),
        ry: (face.ry * 0.10).max(2.0),
    }
}

fn sample_bilinear(image: &LinearImage, x: f32, y: f32) -> [f32; 4] {
    let width = image.width as usize;
    let height = image.height as usize;
    let x = x.clamp(0.0, width as f32 - 1.0);
    let y = y.clamp(0.0, height as f32 - 1.0);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let mut out = [0.0f32; 4];
    for (index, weight) in [
        (y0 * width + x0, (1.0 - fx) * (1.0 - fy)),
        (y0 * width + x1, fx * (1.0 - fy)),
        (y1 * width + x0, (1.0 - fx) * fy),
        (y1 * width + x1, fx * fy),
    ] {
        let base = index * 4;
        for (channel, value) in out.iter_mut().enumerate() {
            *value += image.data[base + channel] * weight;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, draw: impl Fn(u32, u32) -> [u8; 3]) -> LinearImage {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [r, g, b] = draw(x, y);
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        image_core::DecodedImage::new(width, height, rgba)
            .unwrap()
            .to_linear()
    }

    fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> document::PortraitEllipse {
        document::PortraitEllipse {
            center_x: cx,
            center_y: cy,
            radius_x: rx,
            radius_y: ry,
            angle: 0.0,
            feather: 0.2,
        }
    }

    fn geometry() -> document::PortraitGeometry {
        document::PortraitGeometry {
            face: Some(ellipse(0.5, 0.5, 0.25, 0.35)),
            left_eye: Some(ellipse(0.42, 0.42, 0.05, 0.025)),
            right_eye: Some(ellipse(0.58, 0.42, 0.05, 0.025)),
            mouth: Some(ellipse(0.5, 0.68, 0.08, 0.02)),
            ..Default::default()
        }
    }

    fn luma_at(image: &LinearImage, x: u32, y: u32) -> f32 {
        let index = ((y * image.width + x) * 4) as usize;
        crate::luma(
            image.data[index],
            image.data[index + 1],
            image.data[index + 2],
        )
    }

    #[test]
    fn face_slim_warps_the_lower_face_and_spares_the_background() {
        let mut source = image(160, 160, |x, y| {
            if (88..92).contains(&x) && (70..110).contains(&y) {
                [20, 20, 20]
            } else {
                [208, 168, 140]
            }
        });
        source.data.chunks_exact_mut(4).for_each(|px| {
            px[3] = 1.0;
        });
        let before = source.data.clone();
        let background_before = luma_at(&source, 2, 2);
        let params = document::PortraitParams {
            face_slim: 100.0,
            geometry: Some(geometry()),
            ..Default::default()
        };
        apply_face_shape(&mut source, &params);
        let changed = before
            .chunks_exact(4)
            .zip(source.data.chunks_exact(4))
            .enumerate()
            .any(|(index, (old, new))| {
                let x = (index % 160) as u32;
                let y = (index / 160) as u32;
                (60..105).contains(&x)
                    && (70..110).contains(&y)
                    && old[..3].iter().zip(&new[..3]).any(|(a, b)| (a - b).abs() > 1e-4)
            });
        assert!(changed, "the lower face must move");
        assert_eq!(luma_at(&source, 2, 2), background_before);
        assert!(source.data.chunks_exact(4).all(|px| px[3] == 1.0));
    }

    #[test]
    fn eye_size_expands_the_iris_area() {
        let mut source = image(160, 160, |x, y| {
            let distance = ((x as f32 - 67.0).powi(2) + (y as f32 - 67.0).powi(2)).sqrt();
            if distance < 4.0 {
                [30, 30, 30]
            } else {
                [208, 168, 140]
            }
        });
        let before = luma_at(&source, 67 + 4, 67);
        let params = document::PortraitParams {
            eye_size: 100.0,
            geometry: Some(geometry()),
            ..Default::default()
        };
        apply_face_shape(&mut source, &params);
        let after = luma_at(&source, 67 + 4, 67);
        assert!(after < before, "the iris should grow: {before} -> {after}");
    }

    #[test]
    fn chin_and_eye_distance_are_independent_of_the_face_oval_only() {
        let mut source = image(160, 160, |x, y| {
            if (112..116).contains(&y) && (60..100).contains(&x) {
                [40, 30, 30]
            } else {
                [208, 168, 140]
            }
        });
        let before = source.data.clone();
        let params = document::PortraitParams {
            chin_length: 100.0,
            eye_distance: 80.0,
            geometry: Some(geometry()),
            ..Default::default()
        };
        apply_face_shape(&mut source, &params);
        let changed = before
            .chunks_exact(4)
            .zip(source.data.chunks_exact(4))
            .enumerate()
            .any(|(index, (old, new))| {
                let x = (index % 160) as u32;
                let y = (index / 160) as u32;
                (40..120).contains(&x)
                    && (100..140).contains(&y)
                    && old[..3].iter().zip(&new[..3]).any(|(a, b)| (a - b).abs() > 1e-4)
            });
        assert!(changed, "the chin should move");
        assert_eq!(
            source.data[..4],
            before[..4],
            "the background corner stays untouched"
        );
    }

    #[test]
    fn shape_without_geometry_is_a_noop() {
        let mut source = image(32, 32, |_, _| [120, 120, 120]);
        let before = source.data.clone();
        let params = document::PortraitParams {
            face_slim: 100.0,
            ..Default::default()
        };
        apply_face_shape(&mut source, &params);
        assert_eq!(source.data, before);
    }

    #[test]
    fn body_shape_narrows_masked_content_toward_its_center() {
        let mut image = image(120, 80, |x, _| {
            if (40..80).contains(&x) {
                [255, 255, 255]
            } else {
                [0, 0, 0]
            }
        });
        let mut coverage = vec![0.0f32; 120 * 80];
        for y in 10..70 {
            for x in 20..100 {
                coverage[y * 120 + x] = 1.0;
            }
        }
        let params = document::PortraitParams {
            body_slim: 100.0,
            ..Default::default()
        };
        assert!(apply_body_shape(&mut image, &params, &coverage));
        let stripe = (0..120)
            .filter(|x| image.data[(40 * 120 + x) * 4] > 0.5)
            .count();
        assert!(
            (20..38).contains(&stripe),
            "the 40px stripe should compress to roughly 26px, got {stripe}"
        );
    }

    #[test]
    fn body_shape_is_a_no_op_without_strength_coverage_or_face_distortion() {
        let mut image = image(64, 64, |x, y| {
            if (x + y) % 2 == 0 {
                [220, 180, 160]
            } else {
                [80, 60, 50]
            }
        });
        let original = image.data.clone();
        let coverage = vec![1.0f32; 64 * 64];
        let params = document::PortraitParams::default();
        assert!(!apply_body_shape(&mut image, &params, &coverage));
        assert_eq!(image.data, original);

        let slim = document::PortraitParams {
            body_slim: 100.0,
            ..Default::default()
        };
        assert!(!apply_body_shape(&mut image, &slim, &[]));
        assert_eq!(image.data, original);

        let protected = document::PortraitParams {
            geometry: Some(geometry()),
            ..slim
        };
        assert!(apply_body_shape(&mut image, &protected, &coverage));
        // The face ellipse (center 0.5,0.5, radius 0.25x0.35) must stay intact.
        for y in 28..36 {
            for x in 28..36 {
                let base = (y * 64 + x) * 4;
                assert_eq!(
                    image.data[base..base + 4],
                    original[base..base + 4],
                    "pixel ({x},{y}) inside the face moved"
                );
            }
        }
    }

    #[test]
    fn body_lengthen_stretches_the_lower_masked_half() {
        let mut image = image(80, 120, |_, y| {
            if (90..93).contains(&y) {
                [255, 255, 255]
            } else {
                [10, 10, 10]
            }
        });
        let mut coverage = vec![0.0f32; 80 * 120];
        for y in 30..110 {
            for x in 10..70 {
                coverage[y * 80 + x] = 1.0;
            }
        }
        let params = document::PortraitParams {
            body_lengthen: 100.0,
            ..Default::default()
        };
        assert!(apply_body_shape(&mut image, &params, &coverage));
        let brightest_row = (0..120)
            .max_by(|a, b| {
                let left = image.data[(*a as usize * 80 + 40) * 4];
                let right = image.data[(*b as usize * 80 + 40) * 4];
                left.partial_cmp(&right).unwrap()
            })
            .unwrap();
        assert!(
            brightest_row > 93,
            "the lower band should move down, brightest row {brightest_row}"
        );
        // The upper mask boundary itself is outside the stretch and stays put.
        let top = image.data[(20 * 80 + 40) * 4];
        assert!(top < 0.01);
    }
}
