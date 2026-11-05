//! Region-targeted fine repairs: eyes, teeth, under-eye circles, wrinkles and
//! skin defects.
//!
//! Every effect needs measured facial geometry. The renderer persists that
//! geometry with the portrait layer, so nothing here runs detection during a
//! preview. Without geometry only the skin-defect pass can run, because it can
//! fall back to the heuristic skin mask instead of guessing eye positions.

use document::{PortraitEllipse, PortraitGeometry, PortraitParams};
use image_core::{box_blur, linear_to_srgb, srgb_to_linear, LinearImage};

use crate::{clamp01, luma, rgb_to_ycbcr, smoothstep, ycbcr_to_rgb};

/// Soft ellipse after it has been mapped into render pixels.
#[derive(Debug, Clone, Copy)]
struct Shape {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    sin: f32,
    cos: f32,
    inner: f32,
}

impl Shape {
    fn from_ellipse(ellipse: &PortraitEllipse, width: u32, height: u32) -> Option<Self> {
        let rx = ellipse.radius_x * width as f32;
        let ry = ellipse.radius_y * height as f32;
        if !rx.is_finite() || !ry.is_finite() || rx < 0.75 || ry < 0.75 {
            return None;
        }
        let (sin, cos) = ellipse.angle.sin_cos();
        Some(Self {
            cx: ellipse.center_x * width as f32,
            cy: ellipse.center_y * height as f32,
            rx,
            ry,
            sin,
            cos,
            inner: 1.0 - ellipse.feather.clamp(0.05, 0.95),
        })
    }

    fn from_pixels(cx: f32, cy: f32, rx: f32, ry: f32, feather: f32) -> Option<Self> {
        if !cx.is_finite() || !cy.is_finite() || rx < 0.75 || ry < 0.75 {
            return None;
        }
        Some(Self {
            cx,
            cy,
            rx,
            ry,
            sin: 0.0,
            cos: 1.0,
            inner: 1.0 - feather.clamp(0.05, 0.95),
        })
    }

    fn bounds(&self, width: usize, height: usize) -> Option<(usize, usize, usize, usize)> {
        let half_x = ((self.rx * self.cos).powi(2) + (self.ry * self.sin).powi(2)).sqrt();
        let half_y = ((self.rx * self.sin).powi(2) + (self.ry * self.cos).powi(2)).sqrt();
        let left = (self.cx - half_x).floor().max(0.0) as usize;
        let right = (self.cx + half_x).ceil().min(width as f32 - 1.0).max(0.0) as usize;
        let top = (self.cy - half_y).floor().max(0.0) as usize;
        let bottom = (self.cy + half_y).ceil().min(height as f32 - 1.0).max(0.0) as usize;
        if left > right || top > bottom {
            None
        } else {
            Some((left, right, top, bottom))
        }
    }

    fn alpha(&self, x: usize, y: usize) -> f32 {
        let dx = x as f32 + 0.5 - self.cx;
        let dy = y as f32 + 0.5 - self.cy;
        let along = (dx * self.cos + dy * self.sin) / self.rx;
        let across = (-dx * self.sin + dy * self.cos) / self.ry;
        let distance = (along * along + across * across).sqrt();
        if distance >= 1.0 {
            0.0
        } else {
            1.0 - smoothstep(self.inner, 1.0, distance)
        }
    }
}

struct Features {
    left_eye: Option<Shape>,
    right_eye: Option<Shape>,
    left_brow: Option<Shape>,
    right_brow: Option<Shape>,
    mouth: Option<Shape>,
    lips: Option<Shape>,
    teeth: Option<Shape>,
}

struct Geometry {
    face: Option<Shape>,
    neck: Option<Shape>,
    features: Features,
}

impl Geometry {
    fn new(geometry: &PortraitGeometry, width: u32, height: u32) -> Self {
        let shape = |value: Option<PortraitEllipse>| {
            value.and_then(|ellipse| Shape::from_ellipse(&ellipse, width, height))
        };
        Self {
            face: shape(geometry.face),
            neck: shape(geometry.neck),
            features: Features {
                left_eye: shape(geometry.left_eye),
                right_eye: shape(geometry.right_eye),
                left_brow: shape(geometry.left_brow),
                right_brow: shape(geometry.right_brow),
                mouth: shape(geometry.mouth),
                lips: shape(geometry.lips.or(geometry.mouth)),
                teeth: shape(geometry.teeth),
            },
        }
    }
}

/// Pre-rasterized coverage of eyes, brows, mouth and teeth. Defect and wrinkle
/// passes read it so they never bleach an iris, eyebrow or lip.
fn build_guard(geometry: &Geometry, width: usize, height: usize) -> Vec<f32> {
    let mut guard = vec![0.0f32; width * height];
    let features = &geometry.features;
    for shape in [
        features.left_eye,
        features.right_eye,
        features.left_brow,
        features.right_brow,
        features.mouth,
        features.teeth,
    ]
    .into_iter()
    .flatten()
    {
        let Some((left, right, top, bottom)) = shape.bounds(width, height) else {
            continue;
        };
        for y in top..=bottom {
            for x in left..=right {
                let alpha = shape.alpha(x, y);
                if alpha > 0.0 {
                    let slot = &mut guard[y * width + x];
                    *slot = slot.max(alpha);
                }
            }
        }
    }
    guard
}

/// Coverage of the selected face only. Body layers subtract this so a face
/// mask and a body mask on the same photo do not fight over the same pixels.
fn build_face_guard(geometry: &Geometry, width: usize, height: usize) -> Vec<f32> {
    let mut guard = vec![0.0f32; width * height];
    let Some(face) = geometry.face else {
        return guard;
    };
    let Some((left, right, top, bottom)) = face.bounds(width, height) else {
        return guard;
    };
    for y in top..=bottom {
        for x in left..=right {
            let alpha = face.alpha(x, y);
            if alpha > 0.0 {
                guard[y * width + x] = alpha;
            }
        }
    }
    guard
}

/// Fill in the features a layer did not measure. A face oval is enough to
/// place eyes, brows and mouth by proportion; measured ellipses always win.
fn resolved_geometry(geometry: Option<&PortraitGeometry>) -> Option<PortraitGeometry> {
    let mut resolved = *geometry?;
    if resolved.face.is_none() {
        let (Some(left), Some(right), Some(mouth)) =
            (resolved.left_eye, resolved.right_eye, resolved.mouth)
        else {
            return None;
        };
        let eye_distance = (left.center_x - right.center_x).abs().max(0.001);
        let eye_y = (left.center_y + right.center_y) * 0.5;
        resolved.face = Some(PortraitEllipse {
            center_x: (left.center_x + right.center_x) * 0.5,
            center_y: (eye_y + mouth.center_y) * 0.5,
            radius_x: (eye_distance * 1.25).max(0.01),
            radius_y: ((mouth.center_y - eye_y).abs() * 1.9).max(eye_distance * 0.8),
            angle: 0.0,
            feather: 0.5,
        });
    }
    let face = resolved.face?;
    let mirrored = |ellipse: PortraitEllipse| PortraitEllipse {
        center_x: 2.0 * face.center_x - ellipse.center_x,
        ..ellipse
    };
    match (resolved.left_eye, resolved.right_eye) {
        (None, None) => {
            let eye = PortraitEllipse {
                center_x: face.center_x - face.radius_x * 0.32,
                center_y: face.center_y - face.radius_y * 0.18,
                radius_x: face.radius_x * 0.16,
                radius_y: face.radius_y * 0.075,
                angle: 0.0,
                feather: 0.45,
            };
            resolved.left_eye = Some(eye);
            resolved.right_eye = Some(mirrored(eye));
        }
        (None, Some(right)) => resolved.left_eye = Some(mirrored(right)),
        (Some(left), None) => resolved.right_eye = Some(mirrored(left)),
        _ => {}
    }
    let left = resolved.left_eye?;
    let right = resolved.right_eye?;
    let eye_y = (left.center_y + right.center_y) * 0.5;
    if resolved.left_brow.is_none() {
        resolved.left_brow = Some(PortraitEllipse {
            center_x: left.center_x,
            center_y: left.center_y - face.radius_y * 0.11,
            radius_x: face.radius_x * 0.22,
            radius_y: face.radius_y * 0.045,
            angle: 0.0,
            feather: 0.5,
        });
    }
    if resolved.right_brow.is_none() {
        resolved.right_brow = Some(PortraitEllipse {
            center_x: right.center_x,
            center_y: right.center_y - face.radius_y * 0.11,
            radius_x: face.radius_x * 0.22,
            radius_y: face.radius_y * 0.045,
            angle: 0.0,
            feather: 0.5,
        });
    }
    if resolved.mouth.is_none() {
        resolved.mouth = Some(PortraitEllipse {
            center_x: face.center_x,
            center_y: eye_y + face.radius_y * 0.62,
            radius_x: face.radius_x * 0.28,
            radius_y: face.radius_y * 0.10,
            angle: 0.0,
            feather: 0.4,
        });
    }
    let mouth = resolved.mouth?;
    if resolved.lips.is_none() {
        resolved.lips = Some(PortraitEllipse {
            radius_x: mouth.radius_x * 0.95,
            radius_y: mouth.radius_y * 0.80,
            ..mouth
        });
    }
    if resolved.teeth.is_none() {
        resolved.teeth = Some(PortraitEllipse {
            center_x: mouth.center_x,
            center_y: mouth.center_y - mouth.radius_y * 0.15,
            radius_x: mouth.radius_x * 0.72,
            radius_y: mouth.radius_y * 0.45,
            angle: 0.0,
            feather: 0.35,
        });
    }
    if resolved.neck.is_none() {
        resolved.neck = Some(PortraitEllipse {
            center_x: face.center_x,
            center_y: face.center_y + face.radius_y * 1.22,
            radius_x: face.radius_x * 0.78,
            radius_y: face.radius_y * 0.34,
            angle: 0.0,
            feather: 0.6,
        });
    }
    Some(resolved)
}

fn shortest(image: &LinearImage) -> f32 {
    image.width.min(image.height) as f32
}

fn write_srgb(image: &mut LinearImage, index: usize, value: [f32; 3]) {
    image.data[index] = srgb_to_linear(clamp01(value[0]));
    image.data[index + 1] = srgb_to_linear(clamp01(value[1]));
    image.data[index + 2] = srgb_to_linear(clamp01(value[2]));
}

/// Natural whitening: lift skin luminance, neutralize yellow, and apply a
/// gentle warm/cool bias. `guard` protects eyes, brows, lips and teeth (or the
/// selected face on a body layer).
fn apply_whiten(
    image: &mut LinearImage,
    mask: &[f32],
    guard: &[f32],
    amount: f32,
    warmth: f32,
    extra_desat: f32,
) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || mask.len() < width * height {
        return;
    }
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let base = mask[index];
            if base <= 1e-4 {
                continue;
            }
            let excluded = guard.get(index).copied().unwrap_or(0.0);
            let weight = base * (1.0 - excluded) * amount;
            if weight <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            let srgb = [
                linear_to_srgb(image.data[pixel]),
                linear_to_srgb(image.data[pixel + 1]),
                linear_to_srgb(image.data[pixel + 2]),
            ]
            .map(clamp01);
            let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
            let desat = (0.30 + 0.45 * extra_desat.clamp(0.0, 1.0)) * weight;
            let luminance = clamp01(
                luminance
                    + (1.0 - luminance) * 0.10 * weight * (1.0 - smoothstep(0.82, 1.0, luminance)),
            );
            let cb = 0.5 + (cb - 0.5) * (1.0 - desat) - warmth * 0.004 * weight;
            let cr = 0.5 + (cr - 0.5) * (1.0 - desat) + warmth * 0.010 * weight;
            let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
            write_srgb(image, pixel, [r, g, b]);
        }
    }
}

/// Warm/rosy tint. Cheeks receive the strongest tint when geometry exists;
/// without geometry the effect stays a subtle all-over skin warmth.
fn apply_rosy(
    image: &mut LinearImage,
    mask: &[f32],
    geometry: Option<&Geometry>,
    guard: &[f32],
    amount: f32,
) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || mask.len() < width * height {
        return;
    }
    let cheeks = geometry
        .and_then(|item| item.face)
        .map(|face| {
            let mut shapes = Vec::new();
            for side in [-1.0f32, 1.0] {
                if let Some(shape) = Shape::from_pixels(
                    face.cx + side * face.rx * 0.42,
                    face.cy + face.ry * 0.12,
                    face.rx * 0.30,
                    face.ry * 0.20,
                    0.8,
                ) {
                    shapes.push(shape);
                }
            }
            shapes
        })
        .unwrap_or_default();
    let bounds = geometry
        .and_then(|item| item.face)
        .and_then(|face| face.bounds(width, height))
        .unwrap_or((0, width - 1, 0, height - 1));
    for y in bounds.2..=bounds.3 {
        for x in bounds.0..=bounds.1 {
            let index = y * width + x;
            let base = mask[index];
            if base <= 1e-4 {
                continue;
            }
            let excluded = guard.get(index).copied().unwrap_or(0.0);
            let cheek = cheeks
                .iter()
                .map(|shape| shape.alpha(x, y))
                .fold(0.0f32, f32::max);
            let weight = base * (1.0 - excluded) * amount * (0.35 + 0.65 * cheek);
            if weight <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            let srgb = [
                linear_to_srgb(image.data[pixel]),
                linear_to_srgb(image.data[pixel + 1]),
                linear_to_srgb(image.data[pixel + 2]),
            ]
            .map(clamp01);
            let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
            let luminance = clamp01(luminance + (1.0 - luminance) * 0.015 * weight);
            let cb = 0.5 + (cb - 0.5) - 0.008 * weight;
            let cr = 0.5 + (cr - 0.5) + 0.035 * weight;
            let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
            write_srgb(image, pixel, [r, g, b]);
        }
    }
}

/// Compress specular highlights on skin (oil/shine control). `exclusion`
/// carries the feature guard on face layers and the face guard on body layers.
fn apply_oil_control(image: &mut LinearImage, mask: &[f32], exclusion: &[f32], amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || mask.len() < width * height {
        return;
    }
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let base = mask[index];
            if base <= 1e-4 {
                continue;
            }
            let excluded = exclusion.get(index).copied().unwrap_or(0.0);
            let base = base * (1.0 - excluded);
            if base <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            let srgb = [
                linear_to_srgb(image.data[pixel]),
                linear_to_srgb(image.data[pixel + 1]),
                linear_to_srgb(image.data[pixel + 2]),
            ]
            .map(clamp01);
            let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
            let chroma = ((cb - 0.5).abs() + (cr - 0.5).abs()) * 2.0;
            let specular =
                smoothstep(0.62, 0.86, luminance) * (1.0 - smoothstep(0.06, 0.18, chroma));
            let weight = base * amount * specular;
            if weight <= 1e-4 {
                continue;
            }
            let luminance = clamp01(luminance - (luminance - 0.60).max(0.0) * 0.65 * weight);
            let cb = 0.5 + (cb - 0.5) * (1.0 - 0.25 * weight);
            let cr = 0.5 + (cr - 0.5) * (1.0 - 0.25 * weight);
            let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
            write_srgb(image, pixel, [r, g, b]);
        }
    }
}

/// Brighten the sclera while leaving the dark iris and pupil alone.
fn apply_eye_white(image: &mut LinearImage, geometry: &Geometry, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    for eye in [geometry.features.left_eye, geometry.features.right_eye]
        .into_iter()
        .flatten()
    {
        let Some((left, right, top, bottom)) = eye.bounds(width, height) else {
            continue;
        };
        for y in top..=bottom {
            for x in left..=right {
                let alpha = eye.alpha(x, y) * amount;
                if alpha <= 1e-4 {
                    continue;
                }
                let pixel = (y * width + x) * 4;
                let srgb = [
                    linear_to_srgb(image.data[pixel]),
                    linear_to_srgb(image.data[pixel + 1]),
                    linear_to_srgb(image.data[pixel + 2]),
                ]
                .map(clamp01);
                let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
                let chroma = ((cb - 0.5).abs() + (cr - 0.5).abs()) * 2.0;
                let sclera =
                    smoothstep(0.45, 0.72, luminance) * (1.0 - smoothstep(0.08, 0.22, chroma));
                let weight = alpha * sclera;
                if weight <= 1e-4 {
                    continue;
                }
                let luminance = clamp01(luminance + (1.0 - luminance) * 0.18 * weight);
                let cb = cb + (0.5 - cb) * 0.35 * weight;
                let cr = cr + (0.5 - cr) * 0.35 * weight;
                let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                write_srgb(image, pixel, [r, g, b]);
            }
        }
    }
}

/// Natural lip tint. The teeth ellipse is excluded so a smile does not turn
/// white teeth pink.
fn apply_lip_tint(image: &mut LinearImage, geometry: &Geometry, amount: f32, warmth: f32) {
    if amount <= 0.0 {
        return;
    }
    let Some(shape) = geometry.features.lips.or(geometry.features.mouth) else {
        return;
    };
    let width = image.width as usize;
    let height = image.height as usize;
    let Some((left, right, top, bottom)) = shape.bounds(width, height) else {
        return;
    };
    let warmth = warmth.clamp(-1.0, 1.0);
    for y in top..=bottom {
        for x in left..=right {
            let alpha = shape.alpha(x, y) * amount;
            if alpha <= 1e-4 {
                continue;
            }
            let teeth = geometry
                .features
                .teeth
                .map(|teeth| teeth.alpha(x, y))
                .unwrap_or(0.0);
            let weight = alpha * (1.0 - teeth * 0.9);
            if weight <= 1e-4 {
                continue;
            }
            let pixel = (y * width + x) * 4;
            let srgb = [
                linear_to_srgb(image.data[pixel]),
                linear_to_srgb(image.data[pixel + 1]),
                linear_to_srgb(image.data[pixel + 2]),
            ]
            .map(clamp01);
            let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
            let cr = cr + (0.045 + 0.030 * warmth) * weight;
            let cb = cb - (0.010 + 0.015 * warmth) * weight;
            let luminance = clamp01(luminance - 0.012 * weight);
            let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
            write_srgb(image, pixel, [r, g, b]);
        }
    }
}

/// Warm eyeshadow on the upper lid. The luminance gate keeps lashes and the
/// eyeball itself out of the tint.
fn apply_eyeshadow(image: &mut LinearImage, geometry: &Geometry, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    for eye in [geometry.features.left_eye, geometry.features.right_eye]
        .into_iter()
        .flatten()
    {
        let Some(shape) = Shape::from_pixels(
            eye.cx,
            eye.cy - eye.ry * 0.85,
            eye.rx * 1.30,
            eye.ry * 0.90,
            0.8,
        ) else {
            continue;
        };
        let Some((left, right, top, bottom)) = shape.bounds(width, height) else {
            continue;
        };
        for y in top..=bottom {
            for x in left..=right {
                let alpha = shape.alpha(x, y) * amount;
                if alpha <= 1e-4 {
                    continue;
                }
                let pixel = (y * width + x) * 4;
                let srgb = [
                    linear_to_srgb(image.data[pixel]),
                    linear_to_srgb(image.data[pixel + 1]),
                    linear_to_srgb(image.data[pixel + 2]),
                ]
                .map(clamp01);
                let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
                let weight = alpha * smoothstep(0.18, 0.36, luminance);
                if weight <= 1e-4 {
                    continue;
                }
                let luminance = clamp01(luminance - 0.045 * weight);
                let cb = cb - 0.012 * weight;
                let cr = cr + 0.032 * weight;
                let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                write_srgb(image, pixel, [r, g, b]);
            }
        }
    }
}

/// Darken and warm the brows so they frame the eyes after smoothing.
fn apply_brow_tint(image: &mut LinearImage, geometry: &Geometry, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    for brow in [geometry.features.left_brow, geometry.features.right_brow]
        .into_iter()
        .flatten()
    {
        let Some((left, right, top, bottom)) = brow.bounds(width, height) else {
            continue;
        };
        for y in top..=bottom {
            for x in left..=right {
                let alpha = brow.alpha(x, y) * amount;
                if alpha <= 1e-4 {
                    continue;
                }
                let pixel = (y * width + x) * 4;
                let srgb = [
                    linear_to_srgb(image.data[pixel]),
                    linear_to_srgb(image.data[pixel + 1]),
                    linear_to_srgb(image.data[pixel + 2]),
                ]
                .map(clamp01);
                let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
                let luminance = clamp01(luminance - 0.10 * alpha);
                let cb = cb - 0.008 * alpha;
                let cr = cr + 0.020 * alpha;
                let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                write_srgb(image, pixel, [r, g, b]);
            }
        }
    }
}

/// Add a subtle nose contour: shadow along both sides and a light bridge.
fn apply_nose_contour(image: &mut LinearImage, geometry: &Geometry, guard: &[f32], amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let (Some(face), Some(left), Some(right)) = (
        geometry.face,
        geometry.features.left_eye,
        geometry.features.right_eye,
    ) else {
        return;
    };
    let width = image.width as usize;
    let height = image.height as usize;
    let eye_y = (left.cy + right.cy) * 0.5;
    let mouth_y = geometry
        .features
        .mouth
        .map(|mouth| mouth.cy)
        .unwrap_or(eye_y + face.ry * 0.62);
    let top = eye_y + (mouth_y - eye_y) * 0.28;
    let bottom = mouth_y - (mouth_y - eye_y) * 0.16;
    let centre_y = (top + bottom) * 0.5;
    let half_height = ((bottom - top) * 0.5).max(2.0);
    let nose_width = (face.rx * 0.22).max(2.0);
    let mut shapes: Vec<(Shape, f32)> = Vec::new();
    for side in [-1.0f32, 1.0] {
        if let Some(shape) = Shape::from_pixels(
            face.cx + side * nose_width * 0.78,
            centre_y,
            nose_width * 0.30,
            half_height * 0.9,
            0.75,
        ) {
            shapes.push((shape, amount));
        }
    }
    if let Some(shape) = Shape::from_pixels(
        face.cx,
        centre_y - half_height * 0.15,
        nose_width * 0.12,
        half_height * 0.65,
        0.8,
    ) {
        shapes.push((shape, -amount * 0.45));
    }
    for (shape, weight) in shapes {
        let Some((left_px, right_px, top_px, bottom_px)) = shape.bounds(width, height) else {
            continue;
        };
        for y in top_px..=bottom_px {
            for x in left_px..=right_px {
                let alpha = shape.alpha(x, y);
                if alpha <= 1e-4 {
                    continue;
                }
                let index = y * width + x;
                let guarded = guard.get(index).copied().unwrap_or(0.0);
                let w = alpha * weight * (1.0 - guarded * 0.5);
                if w.abs() <= 1e-4 {
                    continue;
                }
                let pixel = index * 4;
                let srgb = [
                    linear_to_srgb(image.data[pixel]),
                    linear_to_srgb(image.data[pixel + 1]),
                    linear_to_srgb(image.data[pixel + 2]),
                ]
                .map(clamp01);
                let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
                let luminance = clamp01(luminance - 0.035 * w);
                let cb = cb + 0.004 * w;
                let cr = cr - 0.004 * w;
                let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                write_srgb(image, pixel, [r, g, b]);
            }
        }
    }
}

/// Local contrast and a gentle shadow lift inside each measured eye ellipse.
fn apply_eye_enhance(image: &mut LinearImage, geometry: &Geometry, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 {
        return;
    }
    let radius = ((shortest(image) / 900.0).round() as u32).clamp(1, 4);
    let local = box_blur(image, radius);
    for shape in [geometry.features.left_eye, geometry.features.right_eye]
        .into_iter()
        .flatten()
    {
        let Some((left, right, top, bottom)) = shape.bounds(width, height) else {
            continue;
        };
        for y in top..=bottom {
            for x in left..=right {
                let alpha = shape.alpha(x, y) * amount;
                if alpha <= 1e-4 {
                    continue;
                }
                let index = (y * width + x) * 4;
                let base = [
                    image.data[index],
                    image.data[index + 1],
                    image.data[index + 2],
                ];
                let value = luma(base[0], base[1], base[2]);
                if value <= 1e-5 {
                    continue;
                }
                let blurred = luma(
                    local.data[index],
                    local.data[index + 1],
                    local.data[index + 2],
                );
                let detail = ((value - blurred) / value).clamp(-0.8, 0.8);
                let lift =
                    0.12 * alpha * (1.0 - value.clamp(0.0, 1.0)) * smoothstep(0.01, 0.15, value);
                let scale = (1.0 + detail * 0.50 * alpha + lift).clamp(0.65, 1.45);
                let scaled = [base[0] * scale, base[1] * scale, base[2] * scale]
                    .map(linear_to_srgb)
                    .map(clamp01);
                let (luminance, cb, cr) = rgb_to_ycbcr(scaled[0], scaled[1], scaled[2]);
                let boost = 1.0 + 0.12 * alpha;
                let cb = 0.5 + (cb - 0.5) * boost;
                let cr = 0.5 + (cr - 0.5) * boost;
                write_srgb(image, index, {
                    let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                    [r, g, b]
                });
            }
        }
    }
}

/// Neutralize the yellow cast of teeth. Saturated lips stay untouched.
fn apply_teeth_whiten(image: &mut LinearImage, geometry: &Geometry, whiten: f32, brighten: f32) {
    if whiten <= 0.0 && brighten <= 0.0 {
        return;
    }
    let Some(shape) = geometry.features.teeth.or(geometry.features.mouth) else {
        return;
    };
    let width = image.width as usize;
    let height = image.height as usize;
    let Some((left, right, top, bottom)) = shape.bounds(width, height) else {
        return;
    };
    for y in top..=bottom {
        for x in left..=right {
            let alpha = shape.alpha(x, y);
            if alpha <= 1e-4 {
                continue;
            }
            let index = (y * width + x) * 4;
            let srgb = [
                linear_to_srgb(image.data[index]),
                linear_to_srgb(image.data[index + 1]),
                linear_to_srgb(image.data[index + 2]),
            ]
            .map(clamp01);
            let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
            let chroma = ((cb - 0.5).abs() + (cr - 0.5).abs()) * 2.0;
            let whiteness =
                smoothstep(0.42, 0.72, luminance) * (1.0 - smoothstep(0.40, 0.60, chroma));
            let weight = alpha * whiteness;
            if weight <= 1e-4 {
                continue;
            }
            let yellow = weight * whiten;
            let lift = weight * (0.10 * whiten + 0.20 * brighten);
            let luminance = luminance + (1.0 - luminance) * lift;
            let cb = cb + (0.5 - cb) * 0.75 * yellow;
            let cr = cr + (0.5 - cr) * 0.75 * yellow;
            let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
            write_srgb(image, index, [r, g, b]);
        }
    }
}

/// Lift the shadow under each eye and pull its bluish cast toward the skin.
///
/// Dark-circle and eye-bag corrections share one weight map so enabling both
/// never applies the same lift twice.
fn apply_under_eye(
    image: &mut LinearImage,
    geometry: &Geometry,
    guard: &[f32],
    dark_amount: f32,
    bag_amount: f32,
    skin_mean: Option<(f32, f32, f32)>,
) {
    if dark_amount <= 0.0 && bag_amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 {
        return;
    }
    let Some((mean_luma, mean_cb, mean_cr)) = skin_mean else {
        return;
    };
    let mut weights = vec![0.0f32; width * height];
    let mut push = |shape: Option<Shape>, amount: f32| {
        let Some(shape) = shape else {
            return;
        };
        if amount <= 0.0 {
            return;
        }
        let Some((left, right, top, bottom)) = shape.bounds(width, height) else {
            return;
        };
        for y in top..=bottom {
            for x in left..=right {
                let alpha = shape.alpha(x, y) * amount;
                if alpha > 0.0 {
                    let slot = &mut weights[y * width + x];
                    *slot = slot.max(alpha);
                }
            }
        }
    };
    for eye in [geometry.features.left_eye, geometry.features.right_eye]
        .into_iter()
        .flatten()
    {
        push(
            Shape::from_pixels(
                eye.cx,
                eye.cy + eye.ry * 1.25,
                eye.rx * 1.45,
                eye.ry * 1.60,
                0.75,
            ),
            dark_amount,
        );
        push(
            Shape::from_pixels(
                eye.cx,
                eye.cy + eye.ry * 1.65,
                eye.rx * 1.65,
                eye.ry * 2.05,
                0.85,
            ),
            bag_amount,
        );
    }
    let radius = ((shortest(image) / 900.0).round() as u32).clamp(1, 4);
    let local = box_blur(image, radius);
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let base = weights[index];
            if base <= 1e-4 {
                continue;
            }
            let excluded = guard.get(index).copied().unwrap_or(0.0);
            let pixel = index * 4;
            let srgb = [
                linear_to_srgb(image.data[pixel]),
                linear_to_srgb(image.data[pixel + 1]),
                linear_to_srgb(image.data[pixel + 2]),
            ]
            .map(clamp01);
            let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
            let darkness = (mean_luma - luminance).max(0.0);
            let weight = (base * (1.0 - excluded * 0.35) * smoothstep(0.006, 0.08, darkness))
                .clamp(0.0, 0.9);
            if weight <= 1e-4 {
                continue;
            }
            let blurred = linear_to_srgb(luma(
                local.data[pixel],
                local.data[pixel + 1],
                local.data[pixel + 2],
            ));
            let luminance = clamp01(
                luminance
                    + (mean_luma - luminance) * 0.35 * weight
                    + (blurred - luminance) * 0.45 * weight,
            );
            let cb = cb + (mean_cb - cb) * 0.35 * weight;
            let cr = cr + (mean_cr - cr) * 0.35 * weight;
            let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
            write_srgb(image, pixel, [r, g, b]);
        }
    }
}

/// Zones where fine lines form, with their independently controlled strengths.
fn face_wrinkle_zones(
    params: &PortraitParams,
    geometry: Option<&Geometry>,
    intensity: f32,
) -> Vec<(Shape, f32)> {
    let Some(geometry) = geometry else {
        return Vec::new();
    };
    let (Some(face), Some(left), Some(right), Some(mouth)) = (
        geometry.face,
        geometry.features.left_eye,
        geometry.features.right_eye,
        geometry.features.mouth,
    ) else {
        return Vec::new();
    };
    let eye_distance = (left.cx - right.cx).abs().max(1.0);
    let eye_y = (left.cy + right.cy) * 0.5;
    let vertical = (mouth.cy - eye_y).abs().max(eye_distance * 0.6);
    let mut zones: Vec<(Shape, f32)> = Vec::new();
    let mut push = |shape: Option<Shape>, zone: f32| {
        let amount = zone_strength(params.wrinkle, zone, intensity);
        if let (Some(shape), true) = (shape, amount > 0.0) {
            zones.push((shape, amount));
        }
    };
    push(
        Shape::from_pixels(
            face.cx,
            eye_y - vertical * 0.55,
            eye_distance * 0.85,
            vertical * 0.16,
            0.8,
        ),
        params.wrinkle_forehead,
    );
    push(
        Shape::from_pixels(
            face.cx,
            eye_y - vertical * 0.12,
            eye_distance * 0.18,
            vertical * 0.12,
            0.75,
        ),
        params.wrinkle_glabella,
    );
    for side in [-1.0f32, 1.0] {
        let eye = if side < 0.0 { left } else { right };
        push(
            Shape::from_pixels(
                mouth.cx + mouth.rx * 0.75 * side,
                mouth.cy - mouth.ry * 0.8,
                mouth.rx * 0.35,
                vertical * 0.35,
                0.75,
            ),
            params.wrinkle_nasolabial,
        );
        push(
            Shape::from_pixels(
                eye.cx + eye.rx * 1.2 * side,
                eye.cy,
                eye.rx * 0.9,
                eye.ry * 1.1,
                0.7,
            ),
            params.wrinkle_crow_feet,
        );
        push(
            Shape::from_pixels(
                mouth.cx + mouth.rx * 0.88 * side,
                mouth.cy - mouth.ry * 0.1,
                mouth.rx * 0.24,
                mouth.ry * 1.2,
                0.8,
            ),
            params.wrinkle_mouth_corner,
        );
        push(
            Shape::from_pixels(
                eye.cx,
                eye.cy + eye.ry * 1.55,
                eye.rx * 1.2,
                eye.ry * 0.8,
                0.8,
            ),
            params.wrinkle_under_eye,
        );
    }
    if let Some(neck) = geometry.neck {
        push(Some(neck), params.wrinkle_neck);
    }
    zones
}

/// Suppress dark lines inside the given zones. A shared weight map prevents
/// overlapping zones from being corrected twice.
fn apply_wrinkle_zones(image: &mut LinearImage, zones: &[(Shape, f32)], guard: &[f32]) {
    if zones.is_empty() {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    let mut weights = vec![0.0f32; width * height];
    for (zone, amount) in zones {
        let Some((left_px, right_px, top, bottom)) = zone.bounds(width, height) else {
            continue;
        };
        for y in top..=bottom {
            for x in left_px..=right_px {
                let alpha = zone.alpha(x, y) * amount;
                if alpha > 0.0 {
                    let slot = &mut weights[y * width + x];
                    *slot = slot.max(alpha);
                }
            }
        }
    }
    let radius = ((shortest(image) / 70.0).round() as u32).clamp(4, 24);
    let broad = box_blur(image, radius);
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let weight = weights[index];
            if weight <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            let base = [
                image.data[pixel],
                image.data[pixel + 1],
                image.data[pixel + 2],
            ];
            let value = luma(base[0], base[1], base[2]);
            if value <= 1e-4 {
                continue;
            }
            let blurred = luma(
                broad.data[pixel],
                broad.data[pixel + 1],
                broad.data[pixel + 2],
            );
            let detail = value - blurred;
            if detail >= 0.0 {
                continue;
            }
            let guarded = guard.get(index).copied().unwrap_or(0.0);
            let weight = weight * smoothstep(0.003, 0.02, -detail) * (1.0 - guarded * 0.9);
            if weight <= 1e-4 {
                continue;
            }
            let target = (value + (blurred - value) * 0.85 * weight).max(0.0);
            let scale = (target / value).clamp(0.4, 2.0);
            for (channel, value) in base.iter().enumerate() {
                image.data[pixel + channel] = (value * scale).max(0.0);
            }
        }
    }
}

/// Body smoothing. The selected face is excluded so a body layer does not
/// double-process the face.
fn apply_body_smooth(
    image: &mut LinearImage,
    body: &[f32],
    face_guard: &[f32],
    amount: f32,
    texture: f32,
    radius_scale: f32,
) {
    if amount <= 0.0 {
        return;
    }
    let base_radius = ((shortest(image) / 120.0).round() as u32).clamp(5, 30);
    let radius = ((base_radius as f32) * radius_scale.max(0.4))
        .round()
        .clamp(2.0, 64.0) as u32;
    let low = box_blur(image, radius);
    let high_scale = 0.25 + 0.75 * (texture / 100.0);
    let strength = amount * 0.55;
    let width = image.width as usize;
    let height = image.height as usize;
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let mask = body[index] * (1.0 - face_guard.get(index).copied().unwrap_or(0.0));
            let weight = strength * mask;
            if weight <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            for channel in 0..3 {
                let high = image.data[pixel + channel] - low.data[pixel + channel];
                let target = low.data[pixel + channel] + high * high_scale;
                image.data[pixel + channel] = clamp01(
                    image.data[pixel + channel] + (target - image.data[pixel + channel]) * weight,
                );
            }
        }
    }
}

/// Body even skin, whitening and brightening. All three share the body mask
/// and the selected-face exclusion.
fn apply_body_color(
    image: &mut LinearImage,
    body: &[f32],
    face_guard: &[f32],
    params: &PortraitParams,
    intensity: f32,
) {
    let even = strength(params.body_even, intensity);
    let brighten = strength(params.body_brighten, intensity);
    let whiten = strength(params.body_whiten, intensity);
    let warmth = params.whiten_warmth / 100.0;
    let extra_desat = params.whiten_saturation / 100.0;
    if even <= 0.0 && brighten <= 0.0 && whiten <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    let mut mean_cb = 0.5f32;
    let mut mean_cr = 0.5f32;
    let mut has_mean = false;
    if even > 0.0 {
        let mut weight_sum = 0.0f32;
        let mut cb_sum = 0.0f32;
        let mut cr_sum = 0.0f32;
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                let mask = body[index] * (1.0 - face_guard.get(index).copied().unwrap_or(0.0));
                if mask <= 0.01 {
                    continue;
                }
                let pixel = index * 4;
                let r = clamp01(linear_to_srgb(image.data[pixel]));
                let g = clamp01(linear_to_srgb(image.data[pixel + 1]));
                let b = clamp01(linear_to_srgb(image.data[pixel + 2]));
                let (_, cb, cr) = rgb_to_ycbcr(r, g, b);
                let weight = mask * mask;
                weight_sum += weight;
                cb_sum += cb * weight;
                cr_sum += cr * weight;
            }
        }
        if weight_sum > 1e-6 {
            mean_cb = cb_sum / weight_sum;
            mean_cr = cr_sum / weight_sum;
            has_mean = true;
        }
    }
    let even = if has_mean { even } else { 0.0 };
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let mask = body[index] * (1.0 - face_guard.get(index).copied().unwrap_or(0.0));
            if mask <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            let mut srgb = [
                linear_to_srgb(image.data[pixel]),
                linear_to_srgb(image.data[pixel + 1]),
                linear_to_srgb(image.data[pixel + 2]),
            ]
            .map(clamp01);
            if even > 0.0 {
                let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
                let amount = even * 0.40 * mask;
                let cb = cb + (mean_cb - cb) * amount;
                let cr = cr + (mean_cr - cr) * amount;
                let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                srgb = [clamp01(r), clamp01(g), clamp01(b)];
            }
            if whiten > 0.0 {
                let (luminance, cb, cr) = rgb_to_ycbcr(srgb[0], srgb[1], srgb[2]);
                let weight = whiten * mask;
                let desat = (0.30 + 0.45 * extra_desat.clamp(0.0, 1.0)) * weight;
                let luminance = clamp01(
                    luminance
                        + (1.0 - luminance)
                            * 0.10
                            * weight
                            * (1.0 - smoothstep(0.82, 1.0, luminance)),
                );
                let cb = 0.5 + (cb - 0.5) * (1.0 - desat) - warmth * 0.004 * weight;
                let cr = 0.5 + (cr - 0.5) * (1.0 - desat) + warmth * 0.010 * weight;
                let (r, g, b) = ycbcr_to_rgb(luminance, cb, cr);
                srgb = [clamp01(r), clamp01(g), clamp01(b)];
            }
            let mut linear = [
                srgb_to_linear(srgb[0]),
                srgb_to_linear(srgb[1]),
                srgb_to_linear(srgb[2]),
            ];
            if brighten > 0.0 {
                let luminance = 0.2126 * srgb[0] + 0.7152 * srgb[1] + 0.0722 * srgb[2];
                let rolloff = (1.0 - luminance).clamp(0.0, 1.0);
                let gain = 1.0 + brighten * 0.30 * mask * rolloff * rolloff;
                linear.iter_mut().for_each(|value| *value *= gain);
            }
            image.data[pixel] = clamp01(linear[0]);
            image.data[pixel + 1] = clamp01(linear[1]);
            image.data[pixel + 2] = clamp01(linear[2]);
        }
    }
}

/// Soften small scars, stretch marks and dull patches on the body while
/// keeping the selected face untouched.
fn apply_body_texture(image: &mut LinearImage, body: &[f32], face_guard: &[f32], amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    let radius = ((shortest(image) / 100.0).round() as u32).clamp(6, 36);
    let broad = box_blur(image, radius);
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let mask = body[index] * (1.0 - face_guard.get(index).copied().unwrap_or(0.0));
            if mask <= 1e-4 {
                continue;
            }
            let pixel = index * 4;
            let base = [
                image.data[pixel],
                image.data[pixel + 1],
                image.data[pixel + 2],
            ];
            let value = luma(base[0], base[1], base[2]);
            if value <= 1e-4 {
                continue;
            }
            let blurred = luma(
                broad.data[pixel],
                broad.data[pixel + 1],
                broad.data[pixel + 2],
            );
            let detail = value - blurred;
            if detail >= 0.0 {
                continue;
            }
            let weight = mask * amount * 0.75 * smoothstep(0.003, 0.025, -detail);
            if weight <= 1e-4 {
                continue;
            }
            let target = (value + (blurred - value) * 0.75 * weight).max(0.0);
            let scale = (target / value).clamp(0.4, 2.0);
            for (channel, value) in base.iter().enumerate() {
                image.data[pixel + channel] = (value * scale).max(0.0);
            }
        }
    }
}

/// Remove dark or colour-deviating patches. The small pass handles blemishes
/// and freckles, the mid pass moles, and the large pass birthmarks/tattoos.
fn apply_spots(
    image: &mut LinearImage,
    base_mask: &[f32],
    guard: &[f32],
    focus_face: Option<&Shape>,
    exclusion: Option<&[f32]>,
    amounts: [f32; 3],
) {
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || base_mask.len() < width * height {
        return;
    }
    let shortest = shortest(image);
    let groups = [
        (
            amounts[0],
            ((shortest * 0.006).round() as u32).clamp(4, 10),
            0.006,
        ),
        (
            amounts[1],
            ((shortest * 0.006).round() as u32).clamp(2, 10),
            0.008,
        ),
        (
            amounts[2],
            ((shortest * 0.02).round() as u32).clamp(6, 40),
            0.010,
        ),
    ];
    // Confine the passes to the face oval when one is known; the layer mask
    // already discards everything outside it, and a smaller loop stays fast.
    let (x_from, x_to, y_from, y_to) = match focus_face {
        Some(face) => match face.bounds(width, height) {
            Some((left, right, top, bottom)) => (left, right, top, bottom),
            None => return,
        },
        None => (0, width - 1, 0, height - 1),
    };
    for (amount, radius, threshold) in groups {
        if amount <= 0.0 {
            continue;
        }
        let blurred = box_blur(image, radius);
        for y in y_from..=y_to {
            for x in x_from..=x_to {
                let index = y * width + x;
                let base_weight = match focus_face {
                    Some(face) => {
                        let alpha = face.alpha(x, y);
                        if alpha <= 1e-4 {
                            continue;
                        }
                        // Keep dark moles addressable even when their own
                        // chroma falls outside the skin locus.
                        alpha * (0.55 + 0.45 * base_mask[index])
                    }
                    None => base_mask[index],
                };
                if base_weight <= 1e-4 {
                    continue;
                }
                let guarded = guard.get(index).copied().unwrap_or(0.0);
                let excluded = exclusion
                    .and_then(|values| values.get(index))
                    .copied()
                    .unwrap_or(0.0);
                if guarded >= 0.95 || excluded >= 0.95 {
                    continue;
                }
                let pixel = index * 4;
                let base = [
                    image.data[pixel],
                    image.data[pixel + 1],
                    image.data[pixel + 2],
                ];
                let reference = [
                    blurred.data[pixel],
                    blurred.data[pixel + 1],
                    blurred.data[pixel + 2],
                ];
                let luminance = luma(base[0], base[1], base[2]);
                let blurred_luma = luma(reference[0], reference[1], reference[2]);
                let dark = (blurred_luma - luminance).max(0.0);
                let chroma = ((base[0] - reference[0]).abs()
                    + (base[1] - reference[1]).abs()
                    + (base[2] - reference[2]).abs())
                    / 3.0;
                let defect = dark.max(chroma * 0.9);
                let weight = (base_weight
                    * (1.0 - guarded)
                    * (1.0 - excluded)
                    * amount
                    * smoothstep(threshold, threshold * 5.0, defect))
                .clamp(0.0, 0.95);
                if weight <= 1e-4 {
                    continue;
                }
                for channel in 0..3 {
                    image.data[pixel + channel] =
                        (base[channel] + (reference[channel] - base[channel]) * weight).max(0.0);
                }
            }
        }
    }
}

/// Masks and statistics prepared by the caller for the fine pass.
pub(crate) struct FineMasks<'a> {
    pub face: Option<&'a [f32]>,
    pub body: Option<&'a [f32]>,
    pub mean: Option<(f32, f32, f32)>,
    pub body_mode: bool,
}

fn strength(value: f32, intensity: f32) -> f32 {
    (value / 100.0).clamp(0.0, 1.0) * intensity
}

/// Legacy `wrinkle` applies to every zone; a zone-specific value can raise the
/// strength but never lowers the master.
fn zone_strength(master: f32, zone: f32, intensity: f32) -> f32 {
    let value = if zone > 0.0 { zone.max(master) } else { master };
    strength(value, intensity)
}

/// Apply every enabled fine-repair effect in place. Face layers use the skin
/// mask plus measured geometry; body layers use the body mask and exclude the
/// selected face so the two scopes stay independent.
pub(crate) fn apply_fine_repair(
    image: &mut LinearImage,
    params: &PortraitParams,
    masks: &FineMasks<'_>,
    intensity: f32,
) {
    if !params.has_fine_repair() || intensity <= 0.0 {
        return;
    }
    let geometry = resolved_geometry(params.geometry.as_ref())
        .map(|geometry| Geometry::new(&geometry, image.width, image.height));
    if masks.body_mode {
        if let Some(body) = masks.body {
            apply_body_effects(
                image,
                params,
                body,
                geometry.as_ref(),
                masks.mean,
                intensity,
            );
        }
        return;
    }
    // Eye, teeth, under-eye and wrinkle effects need geometry, not a skin
    // mask, so they still run on a layer whose only enabled effect is one of
    // them. Skin-mask effects below check `face` themselves.
    apply_face_effects(
        image,
        params,
        masks.face,
        geometry.as_ref(),
        masks.mean,
        intensity,
    );
}

fn apply_face_effects(
    image: &mut LinearImage,
    params: &PortraitParams,
    face: Option<&[f32]>,
    geometry: Option<&Geometry>,
    mean: Option<(f32, f32, f32)>,
    intensity: f32,
) {
    let blemish = strength(params.blemish, intensity);
    let freckle = strength(params.freckle, intensity);
    let mole = strength(params.mole, intensity);
    let birthmark = strength(params.birthmark, intensity);
    let tattoo = strength(params.tattoo, intensity);
    let spots = blemish.max(freckle);
    let large = birthmark.max(tattoo);
    let wrinkle_zones = face_wrinkle_zones(params, geometry, intensity);
    let needs_guard = spots > 0.0
        || mole > 0.0
        || large > 0.0
        || !wrinkle_zones.is_empty()
        || strength(params.rosy, intensity) > 0.0
        || strength(params.whiten, intensity) > 0.0
        || strength(params.dark_circle, intensity) > 0.0
        || strength(params.eye_bag, intensity) > 0.0
        || strength(params.oil_control, intensity) > 0.0;
    let guard = if needs_guard {
        geometry
            .map(|geometry| build_guard(geometry, image.width as usize, image.height as usize))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(face) = face {
        if spots > 0.0 || mole > 0.0 || large > 0.0 {
            let focus = geometry.and_then(|item| item.face);
            apply_spots(
                image,
                face,
                &guard,
                focus.as_ref(),
                None,
                [spots, mole, large],
            );
        }
        apply_oil_control(image, face, &guard, strength(params.oil_control, intensity));
    }
    if let Some(geometry) = geometry {
        apply_wrinkle_zones(image, &wrinkle_zones, &guard);
        apply_under_eye(
            image,
            geometry,
            &guard,
            strength(params.dark_circle, intensity),
            strength(params.eye_bag, intensity),
            mean,
        );
        let master = strength(params.eye_enhance, intensity);
        let iris = strength(params.iris_detail, intensity).max(master);
        let white = strength(params.eye_white, intensity).max(master * 0.5);
        apply_teeth_whiten(
            image,
            geometry,
            strength(params.teeth_whiten, intensity),
            strength(params.teeth_brighten, intensity),
        );
        apply_eye_enhance(image, geometry, iris);
        if white > 0.0 {
            apply_eye_white(image, geometry, white);
        }
        apply_lip_tint(
            image,
            geometry,
            strength(params.lip_tint, intensity),
            params.lip_warmth / 100.0,
        );
        apply_eyeshadow(image, geometry, strength(params.eyeshadow, intensity));
        apply_brow_tint(image, geometry, strength(params.brow_tint, intensity));
        apply_nose_contour(
            image,
            geometry,
            &guard,
            strength(params.nose_contour, intensity),
        );
    }
    if let Some(face) = face {
        apply_whiten(
            image,
            face,
            &guard,
            strength(params.whiten, intensity),
            params.whiten_warmth / 100.0,
            params.whiten_saturation / 100.0,
        );
        apply_rosy(
            image,
            face,
            geometry,
            &guard,
            strength(params.rosy, intensity),
        );
    }
}

fn apply_body_effects(
    image: &mut LinearImage,
    params: &PortraitParams,
    body: &[f32],
    geometry: Option<&Geometry>,
    _mean: Option<(f32, f32, f32)>,
    intensity: f32,
) {
    let face_guard = geometry
        .map(|geometry| build_face_guard(geometry, image.width as usize, image.height as usize))
        .unwrap_or_default();
    let smooth = strength(params.body_smooth, intensity);
    if smooth > 0.0 {
        apply_body_smooth(
            image,
            body,
            &face_guard,
            smooth,
            params.texture,
            1.0 + params.smooth_radius / 100.0 * 1.2,
        );
    }
    if params.body_even > 0.0 || params.body_brighten > 0.0 || params.body_whiten > 0.0 {
        apply_body_color(image, body, &face_guard, params, intensity);
    }
    let oil = strength(params.oil_control, intensity);
    if oil > 0.0 {
        apply_oil_control(image, body, &face_guard, oil);
    }
    let blemish = strength(params.body_blemish, intensity);
    if blemish > 0.0 {
        apply_spots(
            image,
            body,
            &[],
            None,
            Some(&face_guard),
            [blemish, 0.0, 0.0],
        );
    }
    let texture = strength(params.body_texture, intensity);
    if texture > 0.0 {
        apply_body_texture(image, body, &face_guard, texture);
    }
    let neck = zone_strength(params.wrinkle, params.wrinkle_neck, intensity);
    if neck > 0.0 {
        if let Some(geometry) = geometry {
            let zones = geometry
                .neck
                .map(|shape| vec![(shape, neck)])
                .unwrap_or_default();
            apply_wrinkle_zones(image, &zones, &[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::PortraitGeometry;

    /// A warm skin field with optional darker/brighter details.
    fn skin_image(width: u32, height: u32, draw: impl Fn(u32, u32) -> [u8; 3]) -> LinearImage {
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

    fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> PortraitEllipse {
        PortraitEllipse {
            center_x: cx,
            center_y: cy,
            radius_x: rx,
            radius_y: ry,
            angle: 0.0,
            feather: 0.2,
        }
    }

    fn base_geometry() -> PortraitGeometry {
        PortraitGeometry {
            face: Some(ellipse(0.5, 0.5, 0.3, 0.4)),
            left_eye: Some(ellipse(0.4, 0.42, 0.05, 0.025)),
            right_eye: Some(ellipse(0.6, 0.42, 0.05, 0.025)),
            left_brow: Some(ellipse(0.4, 0.36, 0.08, 0.015)),
            right_brow: Some(ellipse(0.6, 0.36, 0.08, 0.015)),
            mouth: Some(ellipse(0.5, 0.68, 0.09, 0.03)),
            lips: Some(ellipse(0.5, 0.68, 0.085, 0.024)),
            teeth: Some(ellipse(0.5, 0.675, 0.06, 0.012)),
            neck: None,
        }
    }

    fn masks<'a>(
        face: Option<&'a [f32]>,
        body: Option<&'a [f32]>,
        mean: Option<(f32, f32, f32)>,
        body_mode: bool,
    ) -> FineMasks<'a> {
        FineMasks {
            face,
            body,
            mean,
            body_mode,
        }
    }

    fn sample(image: &LinearImage, x: u32, y: u32) -> [f32; 3] {
        let index = ((y * image.width + x) * 4) as usize;
        [
            image.data[index],
            image.data[index + 1],
            image.data[index + 2],
        ]
    }

    fn luma_at(image: &LinearImage, x: u32, y: u32) -> f32 {
        let pixel = sample(image, x, y);
        luma(pixel[0], pixel[1], pixel[2])
    }

    fn skin_only(width: u32, height: u32) -> LinearImage {
        skin_image(width, height, |_, _| [208, 168, 140])
    }

    #[test]
    fn zero_fine_params_are_a_noop() {
        let mut image = skin_only(64, 64);
        let before = image.data.clone();
        apply_fine_repair(
            &mut image,
            &PortraitParams::default(),
            &masks(None, None, None, false),
            0.78,
        );
        assert_eq!(image.data, before);
    }

    #[test]
    fn eye_enhance_adds_local_contrast_inside_the_eye_only() {
        // Dark iris on a light eye patch; the eye ellipse is at 0.5, 0.5.
        let image = skin_image(120, 120, |x, y| {
            let distance = ((x as f32 - 60.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt();
            if distance < 6.0 {
                [40, 38, 36]
            } else if distance < 10.0 {
                [228, 220, 214]
            } else {
                [208, 168, 140]
            }
        });
        let mut enhanced = image.clone();
        let params = PortraitParams {
            eye_enhance: 100.0,
            geometry: Some(PortraitGeometry {
                left_eye: Some(ellipse(0.5, 0.5, 0.11, 0.09)),
                right_eye: Some(ellipse(0.5, 0.5, 0.11, 0.09)),
                face: Some(ellipse(0.5, 0.5, 0.4, 0.4)),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_fine_repair(&mut enhanced, &params, &masks(None, None, None, false), 1.0);
        let before_edge = (luma_at(&image, 60, 52) - luma_at(&image, 60, 60)).abs();
        let after_edge = (luma_at(&enhanced, 60, 52) - luma_at(&enhanced, 60, 60)).abs();
        assert!(
            after_edge > before_edge,
            "eye contrast should grow: {before_edge} -> {after_edge}"
        );
        assert_eq!(sample(&enhanced, 2, 2), sample(&image, 2, 2));
    }

    #[test]
    fn teeth_whitening_reduces_yellow_and_spares_lips() {
        let mut image = skin_image(120, 120, |_x, y| {
            if (56..64).contains(&y) {
                [226, 214, 138] // yellow teeth
            } else if (64..76).contains(&y) {
                [176, 52, 62] // red lips
            } else {
                [208, 168, 140]
            }
        });
        let before_teeth = sample(&image, 60, 60);
        let before_lips = sample(&image, 60, 70);
        let params = PortraitParams {
            teeth_whiten: 100.0,
            geometry: Some(PortraitGeometry {
                teeth: Some(ellipse(0.5, 0.5, 0.045, 0.012)),
                mouth: Some(ellipse(0.5, 0.55, 0.09, 0.06)),
                ..base_geometry()
            }),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        let after_teeth = sample(&image, 60, 60);
        let after_lips = sample(&image, 60, 70);
        let yellow_before = before_teeth[0] - before_teeth[2];
        let yellow_after = after_teeth[0] - after_teeth[2];
        assert!(
            yellow_after < yellow_before * 0.8,
            "yellow cast should shrink: {yellow_before} -> {yellow_after}"
        );
        assert_eq!(after_lips, before_lips);
    }

    #[test]
    fn dark_circles_brighten_the_shadow_under_the_eye() {
        let mut image = skin_image(120, 140, |_x, y| {
            if (56..66).contains(&y) {
                [150, 120, 120]
            } else {
                [208, 168, 140]
            }
        });
        let before = luma_at(&image, 48, 60);
        let params = PortraitParams {
            dark_circle: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        let skin = crate::skin_mask(&image);
        let mean = (linear_to_srgb(luma_at(&image, 10, 10)), 0.42, 0.58);
        apply_fine_repair(
            &mut image,
            &params,
            &masks(Some(&skin), None, Some(mean), false),
            1.0,
        );
        let after = luma_at(&image, 48, 60);
        assert!(
            after > before * 1.08,
            "shadow should lift: {before} -> {after}"
        );
    }

    #[test]
    fn blemish_repair_reduces_a_dark_spot_inside_the_face() {
        let mut image = skin_image(120, 120, |x, y| {
            let distance = ((x as f32 - 60.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt();
            if distance < 3.0 {
                [120, 90, 80]
            } else {
                [208, 168, 140]
            }
        });
        let surrounding = luma_at(&image, 80, 60);
        let before = (surrounding - luma_at(&image, 60, 60)).abs();
        let params = PortraitParams {
            blemish: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        let skin = crate::skin_mask(&image);
        apply_fine_repair(
            &mut image,
            &params,
            &masks(Some(&skin), None, None, false),
            1.0,
        );
        let after = (surrounding - luma_at(&image, 60, 60)).abs();
        assert!(
            after < before * 0.6,
            "spot contrast should drop: {before} -> {after}"
        );
        assert_eq!(sample(&image, 5, 5), sample(&skin_only(120, 120), 5, 5));
    }

    #[test]
    fn whitening_lifts_skin_and_reduces_yellow() {
        let mut image = skin_image(120, 120, |_, _| [210, 170, 120]);
        let before = sample(&image, 60, 72);
        let skin = crate::skin_mask(&image);
        let params = PortraitParams {
            whiten: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(
            &mut image,
            &params,
            &masks(Some(&skin), None, None, false),
            1.0,
        );
        let after = sample(&image, 60, 72);
        let before_luma = luma(before[0], before[1], before[2]);
        let after_luma = luma(after[0], after[1], after[2]);
        assert!(after_luma > before_luma, "skin should brighten");
        assert!(
            after[0] - after[2] < before[0] - before[2],
            "yellow cast should shrink"
        );
    }

    #[test]
    fn rosy_tints_the_cheeks_more_than_the_centre() {
        let mut image = skin_image(120, 120, |_, _| [208, 168, 140]);
        let skin = crate::skin_mask(&image);
        let params = PortraitParams {
            rosy: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        let cheek_before = sample(&image, 75, 65);
        let center_before = sample(&image, 60, 60);
        apply_fine_repair(
            &mut image,
            &params,
            &masks(Some(&skin), None, None, false),
            1.0,
        );
        let cheek_after = sample(&image, 75, 65);
        let center_after = sample(&image, 60, 60);
        let cheek_gain = (cheek_after[0] - cheek_after[1]) - (cheek_before[0] - cheek_before[1]);
        let center_gain =
            (center_after[0] - center_after[1]) - (center_before[0] - center_before[1]);
        assert!(cheek_gain > center_gain, "cheeks should receive more tint");
        assert!(cheek_gain > 0.01, "the tint should be visible");
    }

    #[test]
    fn eye_bag_lifts_the_lower_shadow() {
        let mut image = skin_image(120, 140, |_x, y| {
            if (60..70).contains(&y) {
                [150, 118, 118]
            } else {
                [208, 168, 140]
            }
        });
        let before = luma_at(&image, 48, 65);
        let params = PortraitParams {
            eye_bag: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        let skin = crate::skin_mask(&image);
        let mean = (linear_to_srgb(luma_at(&image, 10, 10)), 0.42, 0.58);
        apply_fine_repair(
            &mut image,
            &params,
            &masks(Some(&skin), None, Some(mean), false),
            1.0,
        );
        let after = luma_at(&image, 48, 65);
        assert!(after > before * 1.05, "lower shadow should lift");
    }

    #[test]
    fn wrinkle_zones_are_independent() {
        // Two horizontal dark lines: one in the forehead zone, one in the
        // glabella zone. Only the forehead switch is enabled.
        let mut image = skin_image(160, 160, |x, y| {
            if (60..100).contains(&x) && (y == 46 || y == 62) {
                [150, 120, 105]
            } else {
                [208, 168, 140]
            }
        });
        let forehead_before = luma_at(&image, 80, 46);
        let glabella_before = luma_at(&image, 80, 62);
        let params = PortraitParams {
            wrinkle_forehead: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        let forehead_after = luma_at(&image, 80, 46);
        let glabella_after = luma_at(&image, 80, 62);
        assert!(
            forehead_after > forehead_before,
            "forehead zone should lift its line"
        );
        assert_eq!(glabella_after, glabella_before);
    }

    #[test]
    fn oil_control_compresses_skin_highlights() {
        let mut image = skin_image(120, 120, |x, y| {
            let distance = ((x as f32 - 60.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt();
            if distance < 8.0 {
                [246, 238, 228]
            } else {
                [208, 168, 140]
            }
        });
        let skin = crate::skin_mask(&image);
        let before = luma_at(&image, 60, 60);
        let normal_before = luma_at(&image, 85, 60);
        let params = PortraitParams {
            oil_control: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(
            &mut image,
            &params,
            &masks(Some(&skin), None, None, false),
            1.0,
        );
        assert!(luma_at(&image, 60, 60) < before, "highlight should drop");
        assert_eq!(luma_at(&image, 85, 60), normal_before);
    }

    #[test]
    fn eye_white_brightens_sclera_but_not_the_iris() {
        let mut image = skin_image(120, 120, |x, y| {
            let distance = ((x as f32 - 60.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt();
            if distance < 2.0 {
                [38, 38, 42]
            } else if distance < 7.0 {
                [230, 230, 236]
            } else {
                [208, 168, 140]
            }
        });
        let sclera_before = luma_at(&image, 64, 60);
        let iris_before = luma_at(&image, 60, 60);
        let params = PortraitParams {
            eye_white: 100.0,
            geometry: Some(PortraitGeometry {
                left_eye: Some(ellipse(0.5, 0.5, 0.09, 0.07)),
                right_eye: Some(ellipse(0.5, 0.5, 0.09, 0.07)),
                face: Some(ellipse(0.5, 0.5, 0.4, 0.4)),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        assert!(luma_at(&image, 64, 60) > sclera_before);
        assert_eq!(luma_at(&image, 60, 60), iris_before);
    }

    #[test]
    fn teeth_brightening_works_without_yellow_removal() {
        let mut image = skin_image(120, 120, |_x, y| {
            if (56..64).contains(&y) {
                [226, 214, 138]
            } else {
                [208, 168, 140]
            }
        });
        let before = sample(&image, 60, 60);
        let params = PortraitParams {
            teeth_brighten: 100.0,
            geometry: Some(PortraitGeometry {
                teeth: Some(ellipse(0.5, 0.5, 0.045, 0.012)),
                mouth: Some(ellipse(0.5, 0.55, 0.09, 0.06)),
                ..base_geometry()
            }),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        let after = sample(&image, 60, 60);
        assert!(luma(after[0], after[1], after[2]) > luma(before[0], before[1], before[2]));
        let ycc = |pixel: [f32; 3]| {
            rgb_to_ycbcr(
                linear_to_srgb(pixel[0]),
                linear_to_srgb(pixel[1]),
                linear_to_srgb(pixel[2]),
            )
        };
        let (_, cb_before, cr_before) = ycc(before);
        let (_, cb_after, cr_after) = ycc(after);
        assert!((cb_after - cb_before).abs() < 0.005);
        assert!((cr_after - cr_before).abs() < 0.005);
    }

    #[test]
    fn lip_tint_warms_lips_and_spares_teeth() {
        let mut image = skin_image(120, 120, |_x, y| {
            if (56..64).contains(&y) {
                [226, 214, 138]
            } else if (64..76).contains(&y) {
                [176, 52, 62]
            } else {
                [208, 168, 140]
            }
        });
        let lips_before = sample(&image, 60, 70);
        let teeth_before = sample(&image, 60, 60);
        let params = PortraitParams {
            lip_tint: 100.0,
            lip_warmth: 60.0,
            geometry: Some(PortraitGeometry {
                teeth: Some(ellipse(0.5, 0.5, 0.045, 0.012)),
                lips: Some(ellipse(0.5, 0.58, 0.09, 0.055)),
                mouth: Some(ellipse(0.5, 0.58, 0.09, 0.06)),
                ..base_geometry()
            }),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        let lips_after = sample(&image, 60, 70);
        let teeth_after = sample(&image, 60, 60);
        assert!(
            lips_after[0] - lips_after[2] > lips_before[0] - lips_before[2],
            "lips should gain warm red"
        );
        assert_eq!(teeth_after, teeth_before);
    }

    #[test]
    fn eyeshadow_tints_the_upper_lid_only() {
        let mut image = skin_only(120, 120);
        let lid_before = sample(&image, 48, 47);
        let cheek_before = luma_at(&image, 80, 60);
        let params = PortraitParams {
            eyeshadow: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        let lid_after = sample(&image, 48, 47);
        assert!(
            luma(lid_after[0], lid_after[1], lid_after[2])
                < luma(lid_before[0], lid_before[1], lid_before[2])
        );
        assert_eq!(luma_at(&image, 80, 60), cheek_before);
    }

    #[test]
    fn brow_tint_darkens_the_brow_ellipse_only() {
        let mut image = skin_only(120, 120);
        let brow_before = luma_at(&image, 48, 43);
        let cheek_before = luma_at(&image, 80, 60);
        let params = PortraitParams {
            brow_tint: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        assert!(luma_at(&image, 48, 43) < brow_before);
        assert_eq!(luma_at(&image, 80, 60), cheek_before);
    }

    #[test]
    fn nose_contour_darkens_the_sides_only() {
        let mut image = skin_only(120, 120);
        let cheek_before = luma_at(&image, 80, 60);
        let params = PortraitParams {
            nose_contour: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        assert!(luma_at(&image, 54, 68) < luma_at(&skin_only(120, 120), 54, 68));
        assert_eq!(luma_at(&image, 80, 60), cheek_before);
    }

    #[test]
    fn body_effects_skip_the_selected_face() {
        // Left half is the face, right half is body skin. Both contain a dark
        // spot; a body-blemish layer must only repair the body spot.
        let mut image = skin_image(160, 160, |x, y| {
            let centre = if x < 80 { 40.0 } else { 120.0 };
            let distance = ((x as f32 - centre).powi(2) + (y as f32 - 80.0).powi(2)).sqrt();
            if distance < 3.0 {
                [120, 90, 80]
            } else {
                [208, 168, 140]
            }
        });
        let face_surrounding = luma_at(&image, 20, 80);
        let body_surrounding = luma_at(&image, 140, 80);
        let face_before = (face_surrounding - luma_at(&image, 40, 80)).abs();
        let body_before = (body_surrounding - luma_at(&image, 120, 80)).abs();
        let params = PortraitParams {
            body_mode: true,
            body_blemish: 100.0,
            geometry: Some(PortraitGeometry {
                face: Some(ellipse(0.25, 0.5, 0.18, 0.32)),
                ..base_geometry()
            }),
            ..Default::default()
        };
        crate::apply_portrait_adjustments(&mut image, &params);
        let face_after = (face_surrounding - luma_at(&image, 40, 80)).abs();
        let body_after = (body_surrounding - luma_at(&image, 120, 80)).abs();
        assert!(
            (face_after - face_before).abs() < 1e-4,
            "the selected face must stay untouched: {face_before} -> {face_after}"
        );
        assert!(
            body_after < body_before * 0.8,
            "the body spot should be repaired: {body_before} -> {body_after}"
        );
    }

    #[test]
    fn wrinkle_repair_softens_a_dark_line_in_the_forehead_zone() {
        // Forehead zone sits above the eyes around y = 0.29.
        let mut image = skin_image(160, 160, |x, y| {
            if y == 46 && (60..100).contains(&x) {
                [150, 120, 105]
            } else {
                [208, 168, 140]
            }
        });
        let before = luma_at(&image, 80, 46);
        let params = PortraitParams {
            wrinkle: 100.0,
            geometry: Some(base_geometry()),
            ..Default::default()
        };
        apply_fine_repair(&mut image, &params, &masks(None, None, None, false), 1.0);
        let after = luma_at(&image, 80, 46);
        assert!(
            after > before,
            "the dark line should lift: {before} -> {after}"
        );
    }
}
