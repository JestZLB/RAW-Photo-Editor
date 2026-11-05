//! Evidence-driven facial region analysis on top of the face boxes.
//!
//! There is no landmark network in the pure-Rust stack, so keypoints are
//! *measured* from the pixels inside each detected box: the eyes and brows are
//! the darkest symmetric blobs, the mouth is the reddest patch below the nose
//! line, and the face oval comes from a warmth mask. Every region carries a
//! confidence, so the UI must present them as estimates — this is not a
//! per-pixel segmentation model.

use serde::{Deserialize, Serialize};

use crate::analysis::FaceBox;
use image_core::DecodedImage;

/// Soft YCbCr skin locus: Cb 0.30..0.50, Cr 0.52..0.68, with a mid-luma gate so
/// hair, shadows and blown highlights do not count as skin.
fn skin_weight(cb: f32, cr: f32, green: f32, luma: f32) -> f32 {
    let band = |value: f32, low: f32, high: f32, soft: f32| {
        let rising = ((value - (low - soft)) / soft).clamp(0.0, 1.0);
        let falling = 1.0 - ((value - high) / soft).clamp(0.0, 1.0);
        rising.min(falling)
    };
    let chroma = band(cb, 0.30, 0.50, 0.06) * band(cr, 0.52, 0.68, 0.06);
    // Skin keeps some green; deep shadows and neutrals lose it.
    let tone = band(luma, 0.10, 0.98, 0.08) * band(green, 0.06, 0.98, 0.06);
    (chroma * tone).clamp(0.0, 1.0)
}

/// Analysis grid per face. 56 cells localise eyes and mouth on a 200px face
/// while keeping the whole pass in the low milliseconds.
const GRID: usize = 56;
/// Padding around the detector box, in box fractions, so brows and hairline
/// inside the padded window are still addressable.
const MARGIN: f32 = 0.18;
const INNER: f32 = 1.0 - 2.0 * MARGIN;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionPoint {
    /// Source-image pixel coordinates.
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionKind {
    Skin,
    LeftEye,
    RightEye,
    LeftBrow,
    RightBrow,
    Mouth,
    Lips,
    Teeth,
    Nose,
}

impl RegionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Skin => "脸部皮肤",
            Self::LeftEye => "左眼",
            Self::RightEye => "右眼",
            Self::LeftBrow => "左眉",
            Self::RightBrow => "右眉",
            Self::Mouth => "嘴部",
            Self::Lips => "嘴唇",
            Self::Teeth => "牙齿",
            Self::Nose => "鼻部",
        }
    }
}

/// A soft elliptical region, resolution independent so the renderer can
/// rasterize it at any preview or export size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionShape {
    pub center: RegionPoint,
    pub radius_x: f32,
    pub radius_y: f32,
    /// Rotation in radians.
    pub angle: f32,
    /// Edge falloff as a fraction of the radius, 0..1.
    pub feather: f32,
    /// 0..1, how strongly the pixels supported this region.
    pub confidence: f32,
}

impl RegionShape {
    /// Fraction of the detector box the region covers, for the UI summary.
    pub fn area_ratio(&self, box_width: f32, box_height: f32) -> f32 {
        let area = std::f32::consts::PI * self.radius_x * self.radius_y;
        (area / (box_width * box_height).max(1.0)).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRegion {
    pub kind: RegionKind,
    pub shape: RegionShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionKeypoint {
    pub kind: RegionKind,
    pub point: RegionPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceRegions {
    pub face_id: String,
    #[serde(rename = "box")]
    pub face_box: FaceBox,
    pub regions: Vec<DetectedRegion>,
    pub keypoints: Vec<RegionKeypoint>,
    /// 0..1 average confidence across the regions that gate the retouch tools.
    pub score: f32,
}

impl FaceRegions {
    pub fn region(&self, kind: RegionKind) -> Option<&DetectedRegion> {
        self.regions.iter().find(|region| region.kind == kind)
    }

    pub fn point(&self, kind: RegionKind) -> Option<RegionPoint> {
        self.keypoints
            .iter()
            .find(|keypoint| keypoint.kind == kind)
            .map(|keypoint| keypoint.point)
    }
}

#[derive(Debug, Clone, Copy)]
struct Cell {
    luma: f32,
    redness: f32,
    /// 0..1 YCbCr skin-locus membership.
    skin: f32,
}

/// The detector box plus padding, sampled into a fixed grid.
struct FaceWindow {
    origin_x: f32,
    origin_y: f32,
    span_x: f32,
    span_y: f32,
    cells: Vec<Option<Cell>>,
    side: usize,
    /// Image bounds, so no keypoint can be reported outside the photo.
    limit_x: f32,
    limit_y: f32,
}

impl FaceWindow {
    fn at(&self, column: usize, row: usize) -> Option<Cell> {
        self.cells.get(row * self.side + column).copied().flatten()
    }

    fn to_source(&self, u: f32, v: f32) -> RegionPoint {
        RegionPoint {
            x: (self.origin_x + u * self.span_x).clamp(0.0, self.limit_x),
            y: (self.origin_y + v * self.span_y).clamp(0.0, self.limit_y),
        }
    }

    fn to_grid(&self, point: RegionPoint) -> (f32, f32) {
        (
            (point.x - self.origin_x) / self.span_x,
            (point.y - self.origin_y) / self.span_y,
        )
    }

    fn radius_to_source(&self, radius_u: f32, radius_v: f32) -> (f32, f32) {
        (
            (radius_u * self.span_x).max(1.0),
            (radius_v * self.span_y).max(1.0),
        )
    }

    /// Grid row range for a vertical band expressed in window coordinates.
    fn rows(&self, from: f32, to: f32) -> (usize, usize) {
        let first = (from * self.side as f32) as isize;
        let last = (to * self.side as f32) as isize;
        let row_from = first.clamp(0, self.side as isize - 1) as usize;
        let row_to = last.clamp(row_from as isize + 1, self.side as isize) as usize;
        (row_from, row_to)
    }

    fn columns(&self, from: f32, to: f32) -> (usize, usize) {
        let first = (from * self.side as f32) as isize;
        let last = (to * self.side as f32) as isize;
        let column_from = first.clamp(0, self.side as isize - 1) as usize;
        let column_to = last.clamp(column_from as isize + 1, self.side as isize) as usize;
        (column_from, column_to)
    }
}

fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) * 0.5
    }
}

fn sample_window(image: &DecodedImage, face: &FaceBox) -> FaceWindow {
    let side = GRID;
    let span_x = face.width * (1.0 + MARGIN * 2.0);
    let span_y = face.height * (1.0 + MARGIN * 2.0);
    let origin_x = face.x - face.width * MARGIN;
    let origin_y = face.y - face.height * MARGIN;
    let cell_w = (span_x / side as f32).max(1.0);
    let cell_h = (span_y / side as f32).max(1.0);
    let mut cells = vec![None; side * side];

    for row in 0..side {
        for column in 0..side {
            let x0 = (origin_x + column as f32 * cell_w)
                .floor()
                .clamp(0.0, image.width as f32 - 1.0) as u32;
            let y0 = (origin_y + row as f32 * cell_h)
                .floor()
                .clamp(0.0, image.height as f32 - 1.0) as u32;
            let x1 = (x0 as f32 + cell_w)
                .ceil()
                .clamp(x0 as f32 + 1.0, image.width as f32) as u32;
            let y1 = (y0 as f32 + cell_h)
                .ceil()
                .clamp(y0 as f32 + 1.0, image.height as f32) as u32;
            let mut luma = 0.0f64;
            let mut red = 0.0f64;
            let mut green = 0.0f64;
            let mut blue = 0.0f64;
            let mut count = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let index = ((y * image.width + x) * 4) as usize;
                    let r = image.rgba8[index] as f64;
                    let g = image.rgba8[index + 1] as f64;
                    let b = image.rgba8[index + 2] as f64;
                    luma += 0.299 * r + 0.587 * g + 0.114 * b;
                    red += r;
                    green += g;
                    blue += b;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let scale = 1.0 / count as f64;
            let r = red * scale;
            let g = green * scale;
            let b = blue * scale;
            let y = luma * scale;
            // Cb/Cr around the JPEG mid point, normalised back to 0..1.
            let cb = ((b - y) * 0.564 + 128.0) / 255.0;
            let cr = ((r - y) * 0.713 + 128.0) / 255.0;
            cells[row * side + column] = Some(Cell {
                luma: (y / 255.0) as f32,
                redness: cr as f32 - 0.5,
                skin: skin_weight(cb as f32, cr as f32, g as f32 / 255.0, (y / 255.0) as f32),
            });
        }
    }

    FaceWindow {
        origin_x,
        origin_y,
        span_x,
        span_y,
        cells,
        side,
        limit_x: image.width as f32,
        limit_y: image.height as f32,
    }
}

/// Analyze every face box against the decoded source image.
pub fn analyze_regions(image: &DecodedImage, faces: &[FaceBox]) -> Vec<FaceRegions> {
    faces
        .iter()
        .filter_map(|face| analyze_face(image, face))
        .collect()
}

/// Window coordinate of a ratio measured against the detector box itself.
fn axis(ratio: f32) -> f32 {
    MARGIN + ratio * INNER
}

fn analyze_face(image: &DecodedImage, face: &FaceBox) -> Option<FaceRegions> {
    if face.width < 8.0 || face.height < 8.0 || image.width == 0 || image.height == 0 {
        return None;
    }
    let window = sample_window(image, face);
    let skin = skin_ellipse(&window);
    let (left_eye, right_eye) = find_eyes(&window, skin.as_ref())?;
    let brows = find_brows(&window, left_eye, right_eye);
    let mouth = find_mouth(&window, left_eye, right_eye, skin.as_ref());

    let mut regions: Vec<DetectedRegion> = Vec::new();
    let mut keypoints: Vec<RegionKeypoint> = Vec::new();
    let push = |regions: &mut Vec<DetectedRegion>,
                keypoints: &mut Vec<RegionKeypoint>,
                kind: RegionKind,
                center: RegionPoint,
                radius_u: f32,
                radius_v: f32,
                feather: f32,
                confidence: f32| {
        let (radius_x, radius_y) = window.radius_to_source(radius_u, radius_v);
        keypoints.push(RegionKeypoint {
            kind,
            point: center,
        });
        regions.push(DetectedRegion {
            kind,
            shape: RegionShape {
                center,
                radius_x,
                radius_y,
                angle: 0.0,
                feather,
                confidence: confidence.clamp(0.05, 1.0),
            },
        });
    };

    if let Some(shape) = &skin {
        regions.push(DetectedRegion {
            kind: RegionKind::Skin,
            shape: *shape,
        });
    }
    let eye_radius = (left_eye.x - right_eye.x).abs() / window.span_x * 0.42;
    for (kind, point) in [
        (RegionKind::LeftEye, left_eye),
        (RegionKind::RightEye, right_eye),
    ] {
        push(
            &mut regions,
            &mut keypoints,
            kind,
            point,
            eye_radius.max(0.035) * INNER,
            eye_radius.max(0.035) * 0.55 * INNER,
            0.45,
            0.75,
        );
    }
    for (kind, point) in [
        (RegionKind::LeftBrow, brows.0),
        (RegionKind::RightBrow, brows.1),
    ] {
        push(
            &mut regions,
            &mut keypoints,
            kind,
            point,
            eye_radius.max(0.04) * 1.25 * INNER,
            eye_radius.max(0.02) * 0.5 * INNER,
            0.5,
            0.55,
        );
    }
    if let Some(mouth) = &mouth {
        regions.push(DetectedRegion {
            kind: RegionKind::Mouth,
            shape: *mouth,
        });
        keypoints.push(RegionKeypoint {
            kind: RegionKind::Mouth,
            point: mouth.center,
        });
        regions.push(DetectedRegion {
            kind: RegionKind::Lips,
            shape: RegionShape {
                radius_x: mouth.radius_x * 0.95,
                radius_y: mouth.radius_y * 0.8,
                ..*mouth
            },
        });
        if let Some(teeth) = find_teeth(&window, mouth) {
            regions.push(DetectedRegion {
                kind: RegionKind::Teeth,
                shape: teeth,
            });
            keypoints.push(RegionKeypoint {
                kind: RegionKind::Teeth,
                point: teeth.center,
            });
        }
    }
    let eye_mid = RegionPoint {
        x: (left_eye.x + right_eye.x) * 0.5,
        y: (left_eye.y + right_eye.y) * 0.5,
    };
    let mouth_point = mouth
        .map(|shape| shape.center)
        .unwrap_or(window.to_source(0.5, axis(0.68)));
    let nose = RegionPoint {
        x: eye_mid.x,
        y: eye_mid.y + (mouth_point.y - eye_mid.y) * 0.68,
    };
    push(
        &mut regions,
        &mut keypoints,
        RegionKind::Nose,
        nose,
        0.055 * INNER,
        0.075 * INNER,
        0.55,
        0.35,
    );

    let gated: Vec<f32> = regions
        .iter()
        .filter(|region| {
            matches!(
                region.kind,
                RegionKind::LeftEye | RegionKind::RightEye | RegionKind::Mouth | RegionKind::Skin
            )
        })
        .map(|region| region.shape.confidence)
        .collect();
    let score = if gated.is_empty() {
        0.0
    } else {
        gated.iter().sum::<f32>() / gated.len() as f32
    };

    Some(FaceRegions {
        face_id: face.face_id.clone(),
        face_box: face.clone(),
        regions,
        keypoints,
        score,
    })
}

/// Warmth-weighted oval: skin is the part of the window that is redder than it
/// is blue, without being saturated or blown out.
fn skin_ellipse(window: &FaceWindow) -> Option<RegionShape> {
    let mut weights = Vec::new();
    let mut samples = Vec::new();
    for row in 0..window.side {
        for column in 0..window.side {
            let Some(cell) = window.at(column, row) else {
                continue;
            };
            let score = cell.skin;
            if score > 0.35 {
                weights.push(score);
                samples.push((
                    (column as f32 + 0.5) / window.side as f32,
                    (row as f32 + 0.5) / window.side as f32,
                ));
            }
        }
    }
    if samples.len() < window.side * window.side / 14 {
        return None;
    }
    let total: f32 = weights.iter().sum();
    let center_u: f32 = samples
        .iter()
        .zip(&weights)
        .map(|((u, _), weight)| u * weight)
        .sum::<f32>()
        / total;
    let center_v: f32 = samples
        .iter()
        .zip(&weights)
        .map(|((_, v), weight)| v * weight)
        .sum::<f32>()
        / total;
    let var_u: f32 = samples
        .iter()
        .zip(&weights)
        .map(|((u, _), weight)| (u - center_u).powi(2) * weight)
        .sum::<f32>()
        / total;
    let var_v: f32 = samples
        .iter()
        .zip(&weights)
        .map(|((_, v), weight)| (v - center_v).powi(2) * weight)
        .sum::<f32>()
        / total;
    let coverage = total / (window.side * window.side) as f32;
    let (radius_x, radius_y) = window.radius_to_source(
        (var_u.sqrt() * 2.1).clamp(0.06, 0.5),
        (var_v.sqrt() * 2.1).clamp(0.08, 0.55),
    );
    Some(RegionShape {
        center: window.to_source(center_u, center_v),
        radius_x,
        radius_y,
        angle: 0.0,
        feather: 0.35,
        confidence: (coverage * 2.4).clamp(0.2, 0.95),
    })
}

/// A dark blob sitting inside a bright, skin-toned ring.
///
/// Global "darkest column" searches fail on real photos: warm hair and blurred
/// backgrounds are darker *and* pass the chroma skin test. Comparing a cell to
/// the ring around it keeps eyes, brows and nostrils as the only things that
/// are both dark and surrounded by skin.
#[derive(Debug, Clone, Copy)]
struct Blob {
    column: usize,
    row: usize,
    contrast: f32,
}

/// Best dark blob per column inside a band, measured against an annulus whose
/// radii are fractions of the window side.
fn blob_columns(
    window: &FaceWindow,
    band: (f32, f32),
    ring_inner: f32,
    ring_outer: f32,
) -> Vec<Option<Blob>> {
    let side = window.side as isize;
    let (row_from, row_to) = window.rows(band.0, band.1);
    let inner = (ring_inner * side as f32) as isize;
    let outer = ((ring_outer * side as f32) as isize).max(inner + 1);
    (0..window.side)
        .map(|column| {
            let mut best: Option<Blob> = None;
            for row in row_from..row_to {
                let Some(cell) = window.at(column, row) else {
                    continue;
                };
                let mut ring_sum = 0.0f32;
                let mut ring_count = 0usize;
                let mut ring_skin = 0.0f32;
                for dy in -outer..=outer {
                    for dx in -outer..=outer {
                        let distance = dx * dx + dy * dy;
                        if distance < inner * inner || distance > outer * outer {
                            continue;
                        }
                        let neighbour = column as isize + dx;
                        let neighbour_row = row as isize + dy;
                        if neighbour < 0
                            || neighbour_row < 0
                            || neighbour >= side
                            || neighbour_row >= side
                        {
                            continue;
                        }
                        let Some(around) = window.at(neighbour as usize, neighbour_row as usize)
                        else {
                            continue;
                        };
                        ring_sum += around.luma;
                        ring_count += 1;
                        ring_skin = ring_skin.max(around.skin);
                    }
                }
                if ring_count < 12 || ring_skin < 0.45 {
                    continue;
                }
                let contrast = ring_sum / ring_count as f32 - cell.luma;
                if contrast > 0.0 && best.is_none_or(|current| contrast > current.contrast) {
                    best = Some(Blob {
                        column,
                        row,
                        contrast,
                    });
                }
            }
            best
        })
        .collect()
}

/// Strongest blob in a column range.
fn best_blob(blobs: &[Option<Blob>], range: (usize, usize)) -> Option<Blob> {
    let mut best: Option<Blob> = None;
    for blob in blobs.get(range.0..range.1)?.iter().copied().flatten() {
        if best.is_none_or(|current| blob.contrast > current.contrast) {
            best = Some(blob);
        }
    }
    best
}

/// Horizontal extent of the face at a band, measured from skin support. Eyes,
/// brows and mouth are searched inside this span so hair at the temples and the
/// background can never win the "darkest blob" test.
fn skin_extent(window: &FaceWindow, band: (f32, f32)) -> Option<(usize, usize)> {
    let (row_from, row_to) = window.rows(band.0, band.1);
    let mut columns: Vec<usize> = Vec::new();
    for column in 0..window.side {
        let support = (row_from..row_to)
            .filter_map(|row| window.at(column, row))
            .map(|cell| cell.skin)
            .fold(0.0f32, f32::max);
        if support >= 0.5 {
            columns.push(column);
        }
    }
    let (first, last) = (*columns.first()?, *columns.last()?);
    if columns.len() < window.side / 4 || last - first < window.side / 4 {
        return None;
    }
    Some((first, last))
}

/// Left and right search ranges inside the face span, avoiding the temples.
fn inner_ranges(extent: (usize, usize)) -> Option<((usize, usize), (usize, usize))> {
    let span = extent.1.checked_sub(extent.0)?;
    if span < 8 {
        return None;
    }
    let center = extent.0 + span / 2;
    let inset = (span as f32 * 0.16) as usize;
    Some(((extent.0 + inset, center), (center + 1, extent.1 - inset)))
}

fn find_eyes(
    window: &FaceWindow,
    skin: Option<&RegionShape>,
) -> Option<(RegionPoint, RegionPoint)> {
    let band = (axis(0.24), axis(0.64));
    let extent = skin_extent(window, band)?;
    let (left_range, right_range) = inner_ranges(extent)?;
    let blobs = blob_columns(window, band, 0.045, 0.10);
    let left = best_blob(&blobs, left_range)?;
    let right = best_blob(&blobs, right_range)?;
    // Real eyes are distinctly darker than the skin around them.
    if left.contrast < 0.05 || right.contrast < 0.05 {
        return None;
    }
    let center = (extent.0 + extent.1) as f32 * 0.5;
    let symmetry = (left.column + right.column) as f32 * 0.5;
    if (symmetry - center).abs() > (extent.1 - extent.0) as f32 * 0.14 {
        return None;
    }
    // Both eyes sit on the same line; the stronger one anchors the row.
    let row = if left.contrast >= right.contrast {
        left.row
    } else {
        right.row
    };
    let side = window.side;
    let v = (row as f32 + 0.5) / side as f32;
    let left_point = window.to_source((left.column as f32 + 0.5) / side as f32, v);
    let right_point = window.to_source((right.column as f32 + 0.5) / side as f32, v);
    if (left_point.x - right_point.x).abs() < window.span_x * 0.06 {
        return None;
    }
    if let Some(skin) = skin {
        let (_, skin_v) = window.to_grid(skin.center);
        if (skin_v - v).abs() > 0.34 {
            return None;
        }
    }
    Some((left_point, right_point))
}

fn find_brows(
    window: &FaceWindow,
    left_eye: RegionPoint,
    right_eye: RegionPoint,
) -> (RegionPoint, RegionPoint) {
    let side = window.side as f32;
    let (u_left, v_eye) = window.to_grid(left_eye);
    let (u_right, _) = window.to_grid(right_eye);
    let band = (
        (v_eye - 0.13).max(axis(0.04)),
        (v_eye - 0.03).max(axis(0.06)),
    );
    // Brows are wide and flat, so the ring is stretched horizontally.
    let blobs = blob_columns(window, band, 0.05, 0.13);
    let locate = |u_center: f32| -> RegionPoint {
        let range = window.columns(u_center - 0.08, u_center + 0.08);
        let blob = best_blob(&blobs, range);
        match blob {
            Some(blob) => window.to_source(
                (blob.column as f32 + 0.5) / side,
                (blob.row as f32 + 0.5) / side,
            ),
            None => window.to_source(u_center, band.0 + (band.1 - band.0) * 0.5),
        }
    };
    (locate(u_left), locate(u_right))
}

/// The reddest horizontal run below the nose line.
fn find_mouth(
    window: &FaceWindow,
    left_eye: RegionPoint,
    right_eye: RegionPoint,
    skin: Option<&RegionShape>,
) -> Option<RegionShape> {
    let redness: Vec<f32> = window
        .cells
        .iter()
        .filter_map(|cell| cell.map(|c| c.redness))
        .collect();
    let baseline = median(&redness);
    let band = (axis(0.60), axis(0.96));
    let (row_from, row_to) = window.rows(band.0, band.1);
    let eye_center_u = (window.to_grid(left_eye).0 + window.to_grid(right_eye).0) * 0.5;
    let eye_distance = (left_eye.x - right_eye.x).abs() / window.span_x;

    let mut best: Option<(f32, f32, f32, f32)> = None; // center_u, v, half_width, strength
    for row in row_from..row_to {
        let mut sum = 0.0f32;
        let mut moment = 0.0f32;
        let mut min_u = 1.0f32;
        let mut max_u = 0.0f32;
        for column in 0..window.side {
            let Some(cell) = window.at(column, row) else {
                continue;
            };
            let strength = (cell.redness - baseline - 0.015).max(0.0);
            if strength <= 0.0 {
                continue;
            }
            let u = (column as f32 + 0.5) / window.side as f32;
            sum += strength;
            moment += strength * u;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
        }
        if sum <= 0.0 || max_u <= min_u {
            continue;
        }
        let center_u = moment / sum;
        let half_width = ((max_u - min_u) * 0.5).clamp(0.02, 0.3);
        if (center_u - eye_center_u).abs() > 0.2 || half_width < eye_distance * 0.4 {
            continue;
        }
        let strength = sum / (max_u - min_u);
        if best.is_none_or(|(_, _, _, current)| strength > current) {
            best = Some((
                center_u,
                (row as f32 + 0.5) / window.side as f32,
                half_width,
                strength,
            ));
        }
    }
    let (center_u, center_v, half_width, strength) = best?;
    let center = window.to_source(center_u, center_v);
    let (radius_x, radius_y) = window.radius_to_source(half_width * 1.2, 0.045);
    if let Some(skin) = skin {
        let (skin_u, _) = window.to_grid(skin.center);
        if (center_u - skin_u).abs() > 0.25 {
            return None;
        }
    }
    Some(RegionShape {
        center,
        radius_x,
        radius_y,
        angle: 0.0,
        feather: 0.4,
        confidence: (strength * 4.0).clamp(0.2, 0.9),
    })
}

/// Bright, low-chroma cells inside the mouth box are teeth.
fn find_teeth(window: &FaceWindow, mouth: &RegionShape) -> Option<RegionShape> {
    let lumas: Vec<f32> = window
        .cells
        .iter()
        .filter_map(|cell| cell.map(|c| c.luma))
        .collect();
    let face_luma = median(&lumas);
    let (u_center, v_center) = window.to_grid(mouth.center);
    let columns = window.columns(
        u_center - mouth.radius_x / window.span_x,
        u_center + mouth.radius_x / window.span_x,
    );
    let v_span = (mouth.radius_y / window.span_y).max(0.02);
    let rows = window.rows(v_center - v_span, v_center + v_span);
    let mut bright = 0usize;
    let mut total = 0usize;
    for row in rows.0..rows.1 {
        for column in columns.0..columns.1 {
            if let Some(cell) = window.at(column, row) {
                total += 1;
                if cell.luma > face_luma + 0.1 && cell.redness.abs() < 0.06 {
                    bright += 1;
                }
            }
        }
    }
    if total == 0 || bright * 8 < total {
        return None;
    }
    Some(RegionShape {
        center: window.to_source(u_center, v_center - v_span * 0.15),
        radius_x: (mouth.radius_x * 0.7).max(1.0),
        radius_y: (mouth.radius_y * 0.45).max(1.0),
        angle: 0.0,
        feather: 0.35,
        confidence: (bright as f32 / total as f32 * 3.0).clamp(0.15, 0.8),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw a cartoon face: skin oval, dark eyes and brows, a red mouth. All
    /// positions are analytic, so the expected keypoints are known exactly.
    fn synthetic_face(width: u32, height: u32) -> DecodedImage {
        let cx = width as f32 * 0.5;
        let cy = height as f32 * 0.5;
        let rx = width as f32 * 0.28;
        let ry = height as f32 * 0.36;
        let eye_y = cy - ry * 0.24;
        let eye_dx = rx * 0.45;
        let brow_y = eye_y - ry * 0.22;
        let mouth_y = cy + ry * 0.5;
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let inside = |x: f32, y: f32, dx: f32, dy: f32, rx: f32, ry: f32| {
            let nx = (x - dx) / rx;
            let ny = (y - dy) / ry;
            nx * nx + ny * ny <= 1.0
        };
        for y in 0..height {
            for x in 0..width {
                let index = ((y * width + x) * 4) as usize;
                let fx = x as f32;
                let fy = y as f32;
                let mut color = (40.0f32, 60.0f32, 90.0f32);
                if inside(fx, fy, cx, cy, rx, ry) {
                    color = (208.0, 168.0, 140.0);
                }
                if inside(fx, fy, cx - eye_dx, eye_y, rx * 0.17, ry * 0.075)
                    || inside(fx, fy, cx + eye_dx, eye_y, rx * 0.17, ry * 0.075)
                {
                    color = (24.0, 22.0, 22.0);
                }
                if inside(fx, fy, cx - eye_dx, brow_y, rx * 0.24, ry * 0.05)
                    || inside(fx, fy, cx + eye_dx, brow_y, rx * 0.24, ry * 0.05)
                {
                    color = (64.0, 48.0, 38.0);
                }
                if inside(fx, fy, cx, mouth_y, rx * 0.44, ry * 0.09) {
                    color = (176.0, 52.0, 62.0);
                }
                rgba[index] = color.0 as u8;
                rgba[index + 1] = color.1 as u8;
                rgba[index + 2] = color.2 as u8;
                rgba[index + 3] = 255;
            }
        }
        DecodedImage::new(width, height, rgba).unwrap()
    }

    fn face_for(image: &DecodedImage) -> FaceBox {
        FaceBox {
            face_id: "face-1".into(),
            x: image.width as f32 * 0.22,
            y: image.height as f32 * 0.14,
            width: image.width as f32 * 0.56,
            height: image.height as f32 * 0.72,
            confidence: 5.0,
            models: Vec::new(),
        }
    }

    fn analyze(width: u32, height: u32) -> (DecodedImage, FaceBox, FaceRegions) {
        let image = synthetic_face(width, height);
        let face = face_for(&image);
        let regions = analyze_regions(&image, &[face.clone()]);
        assert_eq!(regions.len(), 1, "the synthetic face must be analysed");
        (image, face, regions.into_iter().next().unwrap())
    }

    #[test]
    fn eyes_land_on_the_drawn_blobs() {
        let (image, _, analysis) = analyze(240, 320);
        let left = analysis.point(RegionKind::LeftEye).expect("left eye");
        let right = analysis.point(RegionKind::RightEye).expect("right eye");
        let expected_y = image.height as f32 * 0.5 - image.height as f32 * 0.36 * 0.24;
        let expected_dx = image.width as f32 * 0.28 * 0.45;
        let center_x = image.width as f32 * 0.5;
        let tolerance_x = image.width as f32 * 0.05;
        let tolerance_y = image.height as f32 * 0.05;
        assert!(
            (left.x - (center_x - expected_dx)).abs() < tolerance_x,
            "left eye x {}",
            left.x
        );
        assert!(
            (right.x - (center_x + expected_dx)).abs() < tolerance_x,
            "right eye x {}",
            right.x
        );
        assert!(
            (left.y - expected_y).abs() < tolerance_y,
            "eye y {}",
            left.y
        );
        assert!(
            (right.y - expected_y).abs() < tolerance_y,
            "eye y {}",
            right.y
        );
        assert!(analysis.score > 0.5, "score {}", analysis.score);
    }

    #[test]
    fn mouth_lips_brows_and_skin_match_the_geometry() {
        let (image, face, analysis) = analyze(240, 320);
        let mouth = analysis.region(RegionKind::Mouth).expect("mouth").shape;
        let expected_y = image.height as f32 * 0.5 + image.height as f32 * 0.36 * 0.5;
        assert!(
            (mouth.center.y - expected_y).abs() < image.height as f32 * 0.08,
            "mouth y {}",
            mouth.center.y
        );
        assert!(
            (mouth.center.x - image.width as f32 * 0.5).abs() < image.width as f32 * 0.05,
            "mouth x {}",
            mouth.center.x
        );
        assert!(mouth.radius_x > image.width as f32 * 0.05);
        assert!(analysis.region(RegionKind::Lips).is_some());

        let skin = analysis.region(RegionKind::Skin).expect("skin").shape;
        assert!((skin.center.x - image.width as f32 * 0.5).abs() < image.width as f32 * 0.06);
        assert!(skin.radius_x > image.width as f32 * 0.08);
        assert!(skin.radius_y > image.height as f32 * 0.1);
        assert!(skin.area_ratio(face.width, face.height) > 0.1);

        let brow = analysis.point(RegionKind::LeftBrow).expect("left brow");
        let eye = analysis.point(RegionKind::LeftEye).unwrap();
        assert!(brow.y < eye.y, "brow must sit above the eye");
        assert!(analysis.point(RegionKind::Nose).is_some());
    }

    #[test]
    fn results_scale_to_larger_and_smaller_faces() {
        for (width, height) in [(120, 160), (480, 640)] {
            let (_, _, analysis) = analyze(width, height);
            assert!(
                analysis.point(RegionKind::LeftEye).is_some(),
                "{width}x{height} lost the eyes"
            );
            assert!(
                analysis.region(RegionKind::Mouth).is_some(),
                "{width}x{height} lost the mouth"
            );
            for region in &analysis.regions {
                assert!(region.shape.radius_x >= 1.0);
                assert!(region.shape.radius_y >= 1.0);
                assert!(region.shape.center.x >= 0.0);
                assert!(region.shape.center.x <= width as f32);
                assert!(region.shape.center.y >= 0.0);
                assert!(region.shape.center.y <= height as f32);
            }
        }
    }

    #[test]
    fn flat_and_one_sided_images_yield_nothing() {
        let mut flat = Vec::new();
        for _ in 0..64 * 64 {
            flat.extend_from_slice(&[180, 150, 130, 255]);
        }
        let image = DecodedImage::new(64, 64, flat).unwrap();
        let face = FaceBox {
            face_id: "face-1".into(),
            x: 8.0,
            y: 8.0,
            width: 48.0,
            height: 48.0,
            confidence: 4.0,
            models: Vec::new(),
        };
        assert!(analyze_regions(&image, &[face]).is_empty());

        // One dark blob: no symmetric pair, so the analysis must refuse.
        let mut rgba = vec![200u8; (160 * 200 * 4) as usize];
        for y in 84..104u32 {
            for x in 40..70u32 {
                let index = ((y * 160 + x) * 4) as usize;
                rgba[index] = 15;
                rgba[index + 1] = 15;
                rgba[index + 2] = 15;
            }
        }
        let image = DecodedImage::new(160, 200, rgba).unwrap();
        let face = FaceBox {
            face_id: "face-1".into(),
            x: 20.0,
            y: 20.0,
            width: 120.0,
            height: 160.0,
            confidence: 4.0,
            models: Vec::new(),
        };
        assert!(analyze_regions(&image, &[face]).is_empty());
    }

    #[test]
    fn degenerate_boxes_are_skipped() {
        let image = synthetic_face(120, 160);
        let face = FaceBox {
            face_id: "face-1".into(),
            x: 10.0,
            y: 10.0,
            width: 4.0,
            height: 4.0,
            confidence: 1.0,
            models: Vec::new(),
        };
        assert!(analyze_regions(&image, &[face]).is_empty());
        assert!(analyze_regions(&image, &[]).is_empty());
    }

    #[test]
    fn regions_serialize_with_camel_case_labels() {
        let (_, _, analysis) = analyze(240, 320);
        let json = serde_json::to_value(&analysis).unwrap();
        assert!(json.get("box").is_some());
        assert!(json["regions"][0]["shape"]["radiusX"].is_number());
        assert_eq!(RegionKind::LeftEye.label(), "左眼");
        assert_eq!(RegionKind::Teeth.label(), "牙齿");
    }
}
