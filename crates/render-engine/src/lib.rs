//! CPU reference renderer for the non-destructive layer graph.
//!
//! Missing AI result assets are a hard error (ASSET_MISSING) rather than being
//! silently skipped: dropping an AI layer would make the preview/export differ
//! from the document without telling the user. The app layer surfaces the
//! stable error code and Chinese message.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use color_pipeline::{apply_color_adjustments, apply_lut, Lut};
use document::{
    BlendMode as DocumentBlendMode, CropRect, LayerKind, LayerMask, LiquifyMode, LiquifyStroke,
    MaskGradientKind, MaskStroke, ProjectDocument, RetouchMode, RetouchParams,
};
use image_core::{composite_over, composite_over_masked, BlendMode, DecodedImage, LinearImage};
use portrait_engine::apply_portrait_adjustments;
use thiserror::Error;
use uuid::Uuid;

mod gpu;
pub use gpu::{gpu_available, render_document_gpu};

/// Intermediate renders are linear RGBA f32 (16 bytes/pixel). A tiny crop used
/// to request an 8192px full frame, which can exceed 800MB before the final
/// downscale. Cap the oversampling unless the caller explicitly asked for a
/// larger preview.
const MAX_PREVIEW_RENDER_LONG_EDGE: u32 = 4096;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("AI result asset is missing: {asset_id}")]
    MissingAsset { asset_id: String },
    #[error("failed to load AI result asset {asset_id}: {message}")]
    AssetLoad { asset_id: String, message: String },
    #[error("ai result layer has no asset id")]
    MissingAssetId,
    #[error("lut asset {asset_id} could not be loaded: {message}")]
    LutLoad { asset_id: String, message: String },
    #[error("invalid render dimensions: {0}")]
    InvalidDimensions(String),
}

impl RenderError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingAsset { .. } | Self::MissingAssetId => "ASSET_MISSING",
            Self::AssetLoad { .. } => "ASSET_LOAD_FAILED",
            Self::LutLoad { .. } => "LUT_INVALID",
            Self::InvalidDimensions(_) => "RENDER_FAILED",
        }
    }

    pub fn user_message(&self) -> &'static str {
        match self {
            Self::MissingAsset { .. } | Self::MissingAssetId => {
                "AI 结果图层引用的图片不存在，请重新生成 AI 结果。"
            }
            Self::AssetLoad { .. } => "无法读取 AI 结果图片，请重新生成。",
            Self::LutLoad { .. } => "无法读取 LUT 文件，请重新导入有效的 .cube 文件。",
            Self::InvalidDimensions(_) => "图片尺寸无效，无法渲染。",
        }
    }
}

/// Decoded coverage of a raster (segmentation) mask: one value per pixel in
/// 0..1, row-major, spanning the uncropped source frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterCoverage {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl RasterCoverage {
    pub fn from_values(width: u32, height: u32, values: Vec<f32>) -> Option<Self> {
        if width == 0 || height == 0 || values.len() != width as usize * height as usize {
            return None;
        }
        Some(Self {
            width,
            height,
            values,
        })
    }

    /// Bilinear sample at normalized coordinates, clamped at the edges. The
    /// mask is authored against the source, so this ignores cropping.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let max_x = self.width.saturating_sub(1) as f32;
        let max_y = self.height.saturating_sub(1) as f32;
        let x = u.clamp(0.0, 1.0) * max_x;
        let y = v.clamp(0.0, 1.0) * max_y;
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.width as usize - 1);
        let y1 = (y0 + 1).min(self.height as usize - 1);
        let tx = x - x0 as f32;
        let ty = y - y0 as f32;
        let stride = self.width as usize;
        let at = |xi: usize, yi: usize| self.values[yi * stride + xi];
        let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
        let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

pub trait AssetResolver: Send + Sync {
    fn load_ai_result(&self, asset_id: Uuid) -> Result<DecodedImage, RenderError>;
    fn load_lut(&self, asset_id: Uuid) -> Result<Lut, RenderError>;
    /// Coverage of a raster mask asset. Defaults to "no coverage" so resolvers
    /// without mask support (tests, demo) keep working.
    fn load_raster_mask(&self, _asset_id: Uuid) -> Option<Arc<RasterCoverage>> {
        None
    }
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge1 <= edge0 {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blur_mask_horizontal(input: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let radius = radius.min(width.saturating_sub(1));
    let mut out = vec![0.0f32; input.len()];
    for y in 0..height {
        let mut prefix = vec![0.0f32; width + 1];
        for x in 0..width {
            prefix[x + 1] = prefix[x] + input[y * width + x];
        }
        for x in 0..width {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius).min(width - 1);
            out[y * width + x] = (prefix[hi + 1] - prefix[lo]) / (hi - lo + 1) as f32;
        }
    }
    out
}

fn transpose_mask(input: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; input.len()];
    for y in 0..height {
        for x in 0..width {
            out[x * height + y] = input[y * width + x];
        }
    }
    out
}

fn feather_mask_values(mask: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let horizontal = blur_mask_horizontal(mask, width, height, radius);
    let transposed = transpose_mask(&horizontal, width, height);
    let blurred = blur_mask_horizontal(&transposed, height, width, radius);
    transpose_mask(&blurred, height, width)
}

/// Approximate Gaussian blur: three box passes over RGB, alpha untouched.
///
/// Box passes are separable and use prefix sums, so cost is O(pixels) per pass
/// regardless of radius; three passes are visually indistinguishable from a
/// Gaussian and avoid a kernel-sized inner loop on 4K frames.
fn blur_linear_rgb(image: &mut LinearImage, radius: usize) {
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || radius == 0 {
        return;
    }
    let radius = radius.min(width.min(height) / 2).max(1);
    let mut horizontal = vec![0.0f32; width * height * 3];
    // One buffer for every prefix scan: allocating per row/channel would dominate
    // the cost on a 1600px preview.
    let mut prefix = vec![0.0f32; width.max(height) + 1];
    for _ in 0..3 {
        for y in 0..height {
            for channel in 0..3 {
                prefix[0] = 0.0;
                for x in 0..width {
                    prefix[x + 1] = prefix[x] + image.data[(y * width + x) * 4 + channel];
                }
                for x in 0..width {
                    let left = x.saturating_sub(radius);
                    let right = (x + radius + 1).min(width);
                    horizontal[(y * width + x) * 3 + channel] =
                        (prefix[right] - prefix[left]) / (right - left) as f32;
                }
            }
        }
        for x in 0..width {
            for channel in 0..3 {
                prefix[0] = 0.0;
                for y in 0..height {
                    prefix[y + 1] = prefix[y] + horizontal[(y * width + x) * 3 + channel];
                }
                for y in 0..height {
                    let top = y.saturating_sub(radius);
                    let bottom = (y + radius + 1).min(height);
                    image.data[(y * width + x) * 4 + channel] =
                        (prefix[bottom] - prefix[top]) / (bottom - top) as f32;
                }
            }
        }
    }
}

/// Rasterize normalized brush strokes into the current render resolution.
///
/// Layers are composited on the full frame; cropping happens afterwards. Mask
/// coordinates are normalized to the source, so they map with the full-frame
/// scale and must not subtract the crop offset.
fn rasterize_mask(
    mask: &LayerMask,
    current: &LinearImage,
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    raster: Option<&RasterCoverage>,
) -> Vec<f32> {
    let mut out = if mask.auto_skin && current.width == width && current.height == height {
        let skin = portrait_engine::skin_mask(current);
        let radius = ((width.min(height) as f32 / 400.0).round() as usize).clamp(1, 12);
        feather_mask_values(&skin, width as usize, height as usize, radius)
    } else {
        vec![0.0f32; width as usize * height as usize]
    };
    if width == 0 || height == 0 || source_width == 0 || source_height == 0 {
        return out;
    }
    let scale_x = width as f32 / source_width as f32;
    if let Some(gradient) = &mask.gradient {
        let start_x = gradient.start.x * width as f32;
        let start_y = gradient.start.y * height as f32;
        let end_x = gradient.end.x * width as f32;
        let end_y = gradient.end.y * height as f32;
        let vx = end_x - start_x;
        let vy = end_y - start_y;
        let length_squared = vx * vx + vy * vy;
        if length_squared > 1e-4 {
            let radius = length_squared.sqrt();
            for y in 0..height {
                for x in 0..width {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let t = match gradient.kind {
                        MaskGradientKind::Linear => {
                            ((px - start_x) * vx + (py - start_y) * vy) / length_squared
                        }
                        MaskGradientKind::Radial => {
                            let dx = px - start_x;
                            let dy = py - start_y;
                            (dx * dx + dy * dy).sqrt() / radius
                        }
                    };
                    let value = 1.0 - smoothstep(0.0, 1.0, t.clamp(0.0, 1.0));
                    let index = y as usize * width as usize + x as usize;
                    out[index] = out[index].max(value);
                }
            }
        }
    }
    let shortest = source_width.min(source_height) as f32;
    for stroke in &mask.strokes {
        if stroke.points.is_empty() {
            continue;
        }
        let radius = (stroke.radius * shortest * scale_x).max(0.6);
        let feather = stroke.feather.clamp(0.0, 1.0);
        let inner = radius * (1.0 - feather);
        let mut stamp = |px: f32, py: f32| {
            if px < -radius
                || py < -radius
                || px > width as f32 + radius
                || py > height as f32 + radius
            {
                return;
            }
            let left = (px - radius).floor().max(0.0) as u32;
            let right = (px + radius).ceil().min(width as f32 - 1.0).max(0.0) as u32;
            let top = (py - radius).floor().max(0.0) as u32;
            let bottom = (py + radius).ceil().min(height as f32 - 1.0).max(0.0) as u32;
            if left > right || top > bottom {
                return;
            }
            for y in top..=bottom {
                for x in left..=right {
                    let dx = x as f32 + 0.5 - px;
                    let dy = y as f32 + 0.5 - py;
                    let distance = (dx * dx + dy * dy).sqrt();
                    if distance > radius {
                        continue;
                    }
                    let alpha = 1.0 - smoothstep(inner, radius, distance);
                    let index = y as usize * width as usize + x as usize;
                    if stroke.erase {
                        out[index] = (out[index] - alpha).max(0.0);
                    } else {
                        out[index] = out[index].max(alpha);
                    }
                }
            }
        };
        let mut previous: Option<(f32, f32)> = None;
        for point in &stroke.points {
            let px = point.x * width as f32;
            let py = point.y * height as f32;
            if let Some((prev_x, prev_y)) = previous {
                let dx = px - prev_x;
                let dy = py - prev_y;
                let distance = (dx * dx + dy * dy).sqrt();
                let steps = ((distance / (radius * 0.35)).ceil() as u32).clamp(1, 512);
                for step in 1..steps {
                    let t = step as f32 / steps as f32;
                    stamp(prev_x + dx * t, prev_y + dy * t);
                }
            }
            stamp(px, py);
            previous = Some((px, py));
        }
    }
    for region in &mask.regions {
        let center_x = region.center_x * width as f32;
        let center_y = region.center_y * height as f32;
        let radius_x = (region.radius_x * width as f32).max(0.6);
        let radius_y = (region.radius_y * height as f32).max(0.6);
        let (sin_a, cos_a) = region.angle.sin_cos();
        // Bounding box of the rotated ellipse, not of its axes.
        let half_x = ((radius_x * cos_a).powi(2) + (radius_y * sin_a).powi(2)).sqrt();
        let half_y = ((radius_x * sin_a).powi(2) + (radius_y * cos_a).powi(2)).sqrt();
        let left = (center_x - half_x).floor().max(0.0) as u32;
        let right = (center_x + half_x)
            .ceil()
            .min(width.saturating_sub(1) as f32) as u32;
        let top = (center_y - half_y).floor().max(0.0) as u32;
        let bottom = (center_y + half_y)
            .ceil()
            .min(height.saturating_sub(1) as f32) as u32;
        if left > right || top > bottom {
            continue;
        }
        let inner = 1.0 - region.feather.clamp(0.0, 0.95);
        for y in top..=bottom {
            for x in left..=right {
                // Rotate the offset into the ellipse frame first: dividing
                // before rotating would distort non-circular regions.
                let dx = x as f32 + 0.5 - center_x;
                let dy = y as f32 + 0.5 - center_y;
                let along = (dx * cos_a + dy * sin_a) / radius_x;
                let across = (-dx * sin_a + dy * cos_a) / radius_y;
                let distance = (along * along + across * across).sqrt();
                if distance > 1.0 {
                    continue;
                }
                let alpha = (1.0 - smoothstep(inner, 1.0, distance)) * region.strength;
                let index = y as usize * width as usize + x as usize;
                out[index] = out[index].max(alpha);
            }
        }
    }
    // Pixel coverage from a segmentation provider. Sampling happens on the
    // full frame (mask coordinates are normalized to the source, like strokes),
    // and combines with the vector sources by taking the maximum.
    if let (Some(params), Some(coverage)) = (mask.raster.as_ref(), raster) {
        let threshold = params.threshold.clamp(0.0, 1.0);
        let softness = params.softness.clamp(0.0, 0.499);
        let width_f = width as f32;
        let height_f = height as f32;
        for y in 0..height {
            let v = (y as f32 + 0.5) / height_f;
            for x in 0..width {
                let u = (x as f32 + 0.5) / width_f;
                let value = coverage.sample(u, v);
                let alpha = if softness <= 0.0005 {
                    if value >= threshold {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    smoothstep(threshold - softness, threshold + softness, value)
                };
                if alpha <= 0.0 {
                    continue;
                }
                let index = y as usize * width as usize + x as usize;
                out[index] = out[index].max(alpha);
            }
        }
    }
    if mask.feather > 0.0 {
        let radius =
            ((width.min(height) as f32 * 0.08 * mask.feather).round() as usize).clamp(1, 96);
        out = feather_mask_values(&out, width as usize, height as usize, radius);
    }
    let density = mask.density.clamp(0.0, 1.0);
    for value in &mut out {
        let value_mut = if mask.inverted { 1.0 - *value } else { *value };
        *value = value_mut * density;
    }
    out
}

fn map_blend(mode: DocumentBlendMode) -> BlendMode {
    match mode {
        DocumentBlendMode::Normal => BlendMode::Normal,
        DocumentBlendMode::Multiply => BlendMode::Multiply,
        DocumentBlendMode::Screen => BlendMode::Screen,
        DocumentBlendMode::Overlay => BlendMode::Overlay,
        DocumentBlendMode::SoftLight => BlendMode::SoftLight,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Cpu,
    Wgpu,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Wgpu => "wgpu",
        }
    }
}

/// A full-frame layer at 100% with normal blending is equivalent to applying
/// its adjustments directly to the composited buffer. Masked, feathered or
/// blended layers still need the scratch copy.
fn layer_can_apply_in_place(layer: &document::Layer) -> bool {
    layer.opacity >= 0.999
        && matches!(layer.blend_mode, DocumentBlendMode::Normal)
        && layer
            .mask
            .as_ref()
            .map(|mask| mask.is_empty())
            .unwrap_or(true)
}

pub fn render_document(
    document: &ProjectDocument,
    source: &DecodedImage,
    max_dimension: Option<u32>,
    resolver: &dyn AssetResolver,
) -> Result<LinearImage, RenderError> {
    if source.width == 0 || source.height == 0 {
        return Err(RenderError::InvalidDimensions("source is empty".into()));
    }

    let crop = document.crop.clamped(source.width, source.height);
    // Straightening rotates the full frame first, and the crop rectangle lives in
    // that rotated bounding box.
    let (frame_width, frame_height) =
        CropRect::rotated_frame(source.width, source.height, crop.angle);
    let crop_width = if crop.width == 0 {
        frame_width
    } else {
        crop.width
    };
    let crop_height = if crop.height == 0 {
        frame_height
    } else {
        crop.height
    };
    // Render enough of the full frame so the cropped area still reaches the
    // requested preview/export size.
    let render_max = max_dimension.filter(|max| *max > 0).map(|max| {
        let full_long = frame_width.max(frame_height) as f64;
        let crop_long = crop_width.max(crop_height) as f64;
        let needed = (max as f64 * (full_long / crop_long)).ceil();
        // A tiny crop should not force a full 100MP proxy into memory just to
        // hit a preview size; cap the intermediate render while preserving the
        // explicitly requested size.
        (needed as u32)
            .min(source.width.max(source.height))
            .min(max.max(MAX_PREVIEW_RENDER_LONG_EDGE))
    });
    let mut current = match render_max {
        Some(max) if max > 0 => {
            let proxy = source
                .resize_to_fit(max)
                .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
            proxy.to_linear()
        }
        _ => source.to_linear(),
    };

    let base = document.base_layer();
    if !base.map(|layer| layer.visible).unwrap_or(false) {
        current = LinearImage::transparent(current.width, current.height)
            .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
    }

    let mut lut_cache: HashMap<Uuid, Lut> = HashMap::new();
    // One scratch buffer reused by every adjustment layer: each layer needs the
    // pre-layer pixels intact for compositing, but that does not mean each layer
    // needs its own allocation.
    let mut scratch: Option<LinearImage> = None;
    for layer in &document.layers {
        if layer.kind == LayerKind::Base {
            continue;
        }
        if !layer.visible || layer.opacity <= 0.0 {
            continue;
        }
        match layer.kind {
            LayerKind::Base => {}
            LayerKind::Color => {
                let Some(params) = layer.color else {
                    continue;
                };
                let lut = match params.lut_asset_id.filter(|_| params.uses_lut()) {
                    Some(asset_id) => Some(match lut_cache.entry(asset_id) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => entry.insert(resolver.load_lut(asset_id)?),
                    }),
                    None => None,
                };
                if layer_can_apply_in_place(layer) {
                    // Full-frame normal layer at 100%: adjust the composited
                    // pixels directly instead of cloning a second linear buffer.
                    apply_color_adjustments(&mut current, &params);
                    if let Some(lut) = lut {
                        apply_lut(&mut current, lut, params.lut_strength);
                    }
                    continue;
                }
                let processed = scratch.get_or_insert_with(|| current.clone());
                processed.copy_from(&current);
                apply_color_adjustments(processed, &params);
                if let Some(lut) = lut {
                    apply_lut(processed, lut, params.lut_strength);
                }
                let mask = layer
                    .mask
                    .as_ref()
                    .filter(|mask| !mask.is_empty())
                    .map(|mask| {
                        let raster = mask
                            .raster
                            .as_ref()
                            .and_then(|raster| resolver.load_raster_mask(raster.asset_id));
                        rasterize_mask(
                            mask,
                            &current,
                            source.width,
                            source.height,
                            processed.width,
                            processed.height,
                            raster.as_deref(),
                        )
                    });
                if let Some(mask) = mask {
                    composite_over_masked(
                        &mut current,
                        processed,
                        &mask,
                        layer.opacity,
                        map_blend(layer.blend_mode),
                    )
                    .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
                } else {
                    composite_over(
                        &mut current,
                        processed,
                        layer.opacity,
                        map_blend(layer.blend_mode),
                    )
                    .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
                }
            }
            LayerKind::Blur => {
                let Some(params) = layer.blur else {
                    continue;
                };
                let shortest = current.width.min(current.height) as f32;
                let radius = (params.radius.clamp(0.0, 0.08) * shortest).round();
                if radius < 0.5 {
                    continue;
                }
                let processed = scratch.get_or_insert_with(|| current.clone());
                processed.copy_from(&current);
                blur_linear_rgb(processed, radius as usize);
                let mask = layer
                    .mask
                    .as_ref()
                    .filter(|mask| !mask.is_empty())
                    .map(|mask| {
                        let raster = mask
                            .raster
                            .as_ref()
                            .and_then(|raster| resolver.load_raster_mask(raster.asset_id));
                        rasterize_mask(
                            mask,
                            &current,
                            source.width,
                            source.height,
                            processed.width,
                            processed.height,
                            raster.as_deref(),
                        )
                    });
                if let Some(mask) = mask {
                    composite_over_masked(
                        &mut current,
                        processed,
                        &mask,
                        layer.opacity,
                        map_blend(layer.blend_mode),
                    )
                    .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
                } else {
                    composite_over(
                        &mut current,
                        processed,
                        layer.opacity,
                        map_blend(layer.blend_mode),
                    )
                    .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
                }
            }
            LayerKind::Portrait => {
                let Some(params) = layer.portrait else {
                    continue;
                };
                // Geometry changes the composite itself; applying them to a
                // masked scratch would only reveal the unwarped pixels below.
                if params.has_shape_effect() {
                    portrait_engine::apply_face_shape(&mut current, &params);
                }
                if !params.has_active_effect() {
                    continue;
                }
                // The mask drives both compositing and body reshaping, so it is
                // rasterized once on the pre-layer pixels (auto-skin uses them).
                let coverage = layer
                    .mask
                    .as_ref()
                    .filter(|mask| !mask.is_empty())
                    .map(|mask| {
                        let raster = mask
                            .raster
                            .as_ref()
                            .and_then(|raster| resolver.load_raster_mask(raster.asset_id));
                        rasterize_mask(
                            mask,
                            &current,
                            source.width,
                            source.height,
                            current.width,
                            current.height,
                            raster.as_deref(),
                        )
                    });
                if layer_can_apply_in_place(layer) && coverage.is_none() {
                    apply_portrait_adjustments(&mut current, &params);
                    continue;
                }
                let processed = scratch.get_or_insert_with(|| current.clone());
                processed.copy_from(&current);
                apply_portrait_adjustments(processed, &params);
                if params.body_slim > 0.0 || params.body_lengthen > 0.0 {
                    if let Some(coverage) = coverage.as_deref() {
                        portrait_engine::apply_body_shape(processed, &params, coverage);
                    }
                }
                if let Some(mask) = coverage {
                    composite_over_masked(
                        &mut current,
                        processed,
                        &mask,
                        layer.opacity,
                        map_blend(layer.blend_mode),
                    )
                    .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
                } else {
                    composite_over(
                        &mut current,
                        processed,
                        layer.opacity,
                        map_blend(layer.blend_mode),
                    )
                    .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
                }
            }
            LayerKind::AiResult => {
                let asset_id = layer.asset_id.ok_or(RenderError::MissingAssetId)?;
                let asset = resolver.load_ai_result(asset_id)?;
                // Provider results may not match the canvas ratio. Cover-crop
                // instead of stretching faces to fill the frame.
                let linear = if asset.width == current.width && asset.height == current.height {
                    asset.to_linear()
                } else {
                    asset
                        .resize_cover(current.width, current.height)
                        .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?
                        .to_linear()
                };
                composite_over(
                    &mut current,
                    &linear,
                    layer.opacity,
                    map_blend(layer.blend_mode),
                )
                .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
            }
            LayerKind::Retouch => {
                let Some(params) = &layer.retouch else {
                    continue;
                };
                apply_retouch(
                    &mut current,
                    params,
                    source.width,
                    source.height,
                    layer.opacity,
                );
            }
        }
    }

    if crop.angle.abs() > 0.001 {
        current = rotate_frame(&current, crop.angle)
            .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
    }

    if !crop.is_full(source.width, source.height) {
        let scale_x = current.width as f64 / frame_width as f64;
        let scale_y = current.height as f64 / frame_height as f64;
        let x = ((crop.x as f64) * scale_x).round() as u32;
        let y = ((crop.y as f64) * scale_y).round() as u32;
        let width = (((crop.width as f64) * scale_x).round() as u32).max(1);
        let height = (((crop.height as f64) * scale_y).round() as u32).max(1);
        let x = x.min(current.width.saturating_sub(1));
        let y = y.min(current.height.saturating_sub(1));
        let width = width.min(current.width - x);
        let height = height.min(current.height - y);
        current = current
            .crop(x, y, width, height)
            .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
    }

    if let Some(max) = max_dimension.filter(|max| *max > 0) {
        let long = current.width.max(current.height);
        if long > max {
            // A proxy source plus rounding can yield a crop a few percent over
            // the requested preview size; never return more than asked for.
            let scale = max as f64 / long as f64;
            let width = ((current.width as f64 * scale).round() as u32).max(1);
            let height = ((current.height as f64 * scale).round() as u32).max(1);
            current = current
                .resize_lanczos3(width, height)
                .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
        }
    }

    Ok(current)
}

/// Rotate the whole frame around its centre (clockwise for positive angles) and
/// expand it to the rotated bounding box. Pixels outside the source become
/// transparent; sampling is bilinear on premultiplied alpha so edges do not
/// darken.
fn rotate_frame(image: &LinearImage, angle_degrees: f32) -> Result<LinearImage, RenderError> {
    let radians = angle_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    // Must match `CropRect::rotated_frame`: the crop rectangle is converted
    // with the same bounding box.
    let (out_width, out_height) = CropRect::rotated_frame(image.width, image.height, angle_degrees);
    let mut out = LinearImage::transparent(out_width, out_height)
        .map_err(|e| RenderError::InvalidDimensions(e.to_string()))?;
    let src_cx = (image.width as f32 - 1.0) * 0.5;
    let src_cy = (image.height as f32 - 1.0) * 0.5;
    let dst_cx = (out_width as f32 - 1.0) * 0.5;
    let dst_cy = (out_height as f32 - 1.0) * 0.5;
    let width = image.width as usize;
    let height = image.height as usize;
    for oy in 0..out_height as usize {
        for ox in 0..out_width as usize {
            let dx = ox as f32 - dst_cx;
            let dy = oy as f32 - dst_cy;
            let sx = dx * cos + dy * sin + src_cx;
            let sy = -dx * sin + dy * cos + src_cy;
            if sx < -0.5 || sy < -0.5 || sx > width as f32 - 0.5 || sy > height as f32 - 0.5 {
                continue;
            }
            let x0 = sx.floor();
            let y0 = sy.floor();
            let fx = sx - x0;
            let fy = sy - y0;
            let x0 = x0 as isize;
            let y0 = y0 as isize;
            let mut acc = [0.0f32; 4];
            for (offset_x, offset_y, weight) in [
                (0, 0, (1.0 - fx) * (1.0 - fy)),
                (1, 0, fx * (1.0 - fy)),
                (0, 1, (1.0 - fx) * fy),
                (1, 1, fx * fy),
            ] {
                let x = x0 + offset_x;
                let y = y0 + offset_y;
                if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                    continue;
                }
                let index = (y as usize * width + x as usize) * 4;
                let alpha = image.data[index + 3];
                for (channel, value) in acc.iter_mut().take(3).enumerate() {
                    *value += image.data[index + channel] * alpha * weight;
                }
                acc[3] += alpha * weight;
            }
            if acc[3] <= 0.0 {
                continue;
            }
            let out_index = (oy * out_width as usize + ox) * 4;
            for channel in 0..3 {
                out.data[out_index + channel] = acc[channel] / acc[3];
            }
            out.data[out_index + 3] = acc[3];
        }
    }
    Ok(out)
}

/// Apply liquify strokes in place. Each stroke inverse-maps a snapshot taken
/// before it, so a long drag cannot feed its own output back into the warp.
fn apply_liquify_strokes(
    image: &mut LinearImage,
    strokes: &[LiquifyStroke],
    freeze: &[MaskStroke],
    freeze_grid: &[bool],
    source_width: u32,
    source_height: u32,
    layer_opacity: f32,
) {
    if strokes.is_empty() || image.width == 0 || image.height == 0 {
        return;
    }
    let width = image.width as usize;
    let height = image.height as usize;
    // Painted protection: coverage 1 keeps the pixel completely still.
    let grid_active = freeze_grid.iter().any(|cell| *cell);
    let mut frozen = if freeze.is_empty() {
        Vec::new()
    } else {
        let mask = LayerMask {
            strokes: freeze.to_vec(),
            ..LayerMask::default()
        };
        rasterize_mask(
            &mask,
            image,
            source_width,
            source_height,
            image.width,
            image.height,
            None,
        )
    };
    if grid_active {
        if frozen.is_empty() {
            frozen = vec![0.0f32; width * height];
        }
        for (index, cell) in freeze_grid.iter().enumerate() {
            if !*cell {
                continue;
            }
            let column = index % document::FREEZE_GRID_COLS;
            let row = index / document::FREEZE_GRID_COLS;
            if row >= document::FREEZE_GRID_ROWS {
                break;
            }
            let left = column * width / document::FREEZE_GRID_COLS;
            let right = ((column + 1) * width)
                .div_ceil(document::FREEZE_GRID_COLS)
                .min(width);
            let top = row * height / document::FREEZE_GRID_ROWS;
            let bottom = ((row + 1) * height)
                .div_ceil(document::FREEZE_GRID_ROWS)
                .min(height);
            for y in top..bottom {
                frozen[y * width + left..y * width + right].fill(1.0);
            }
        }
    }
    let scale_x = width as f32 / source_width.max(1) as f32;
    let scale_y = height as f32 / source_height.max(1) as f32;
    let source_shortest = source_width.min(source_height) as f32;
    for stroke in strokes {
        if stroke.points.is_empty() || stroke.strength <= 0.0 {
            continue;
        }
        let radius = (stroke.radius * source_shortest * scale_x).max(1.5);
        let strength = (stroke.strength * layer_opacity).clamp(0.0, 1.0);
        if strength <= 0.0 {
            continue;
        }
        let points: Vec<(f32, f32)> = stroke
            .points
            .iter()
            .map(|point| {
                (
                    point.x * source_width as f32 * scale_x,
                    point.y * source_height as f32 * scale_y,
                )
            })
            .collect();
        let (mut left, mut right, mut top, mut bottom) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for (x, y) in &points {
            left = left.min(x - radius);
            right = right.max(x + radius);
            top = top.min(y - radius);
            bottom = bottom.max(y + radius);
        }
        let left = left.floor().max(0.0) as usize;
        let right = right.ceil().min(width as f32 - 1.0).max(0.0) as usize;
        let top = top.floor().max(0.0) as usize;
        let bottom = bottom.ceil().min(height as f32 - 1.0).max(0.0) as usize;
        if left > right || top > bottom {
            continue;
        }
        let box_width = right - left + 1;
        let box_height = bottom - top + 1;
        let mut source = vec![[0.0f32; 2]; box_width * box_height];
        for y in 0..box_height {
            for x in 0..box_width {
                source[y * box_width + x] =
                    [left as f32 + x as f32 + 0.5, top as f32 + y as f32 + 0.5];
            }
        }
        // Snapshot only the affected region; a local stroke should not clone a
        // full-resolution frame for every drag.
        let mut snapshot = Vec::with_capacity(box_width * box_height * 4);
        for y in top..=bottom {
            let row = (y * width + left) * 4;
            snapshot.extend_from_slice(&image.data[row..row + box_width * 4]);
        }
        for (index, (px, py)) in points.iter().enumerate() {
            let (mut dx, mut dy) = (0.0f32, 0.0f32);
            if stroke.mode == LiquifyMode::Push && points.len() > 1 {
                let (ax, ay) = if index == 0 {
                    points[0]
                } else {
                    points[index - 1]
                };
                let (bx, by) = if index == 0 { points[1] } else { points[index] };
                let length = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
                if length > 1e-3 {
                    dx = (bx - ax) / length;
                    dy = (by - ay) / length;
                }
            }
            let local_left = (px - radius).floor().max(left as f32) as usize;
            let local_right = (px + radius).ceil().min(right as f32) as usize;
            let local_top = (py - radius).floor().max(top as f32) as usize;
            let local_bottom = (py + radius).ceil().min(bottom as f32) as usize;
            for y in local_top..=local_bottom {
                for x in local_left..=local_right {
                    let cx = x as f32 + 0.5;
                    let cy = y as f32 + 0.5;
                    let distance = ((cx - px).powi(2) + (cy - py).powi(2)).sqrt();
                    if distance > radius {
                        continue;
                    }
                    let frozen_here = frozen.get(y * width + x).copied().unwrap_or(0.0);
                    if frozen_here >= 0.99 {
                        continue;
                    }
                    let keep = 1.0 - frozen_here;
                    let falloff = (1.0 - smoothstep(0.0, radius, distance)) * keep;
                    let slot = (y - top) * box_width + (x - left);
                    match stroke.mode {
                        LiquifyMode::Push => {
                            source[slot][0] -= dx * radius * 0.35 * strength * falloff;
                            source[slot][1] -= dy * radius * 0.35 * strength * falloff;
                        }
                        LiquifyMode::Pinch => {
                            let k = 0.35 * strength * falloff;
                            source[slot][0] += (cx - px) * k;
                            source[slot][1] += (cy - py) * k;
                        }
                        LiquifyMode::Bulge => {
                            let k = 0.35 * strength * falloff;
                            source[slot][0] -= (cx - px) * k;
                            source[slot][1] -= (cy - py) * k;
                        }
                    }
                    let offset_x = source[slot][0] - (x as f32 + 0.5);
                    let offset_y = source[slot][1] - (y as f32 + 0.5);
                    let offset = (offset_x * offset_x + offset_y * offset_y).sqrt();
                    let limit = radius * 0.9;
                    if offset > limit && offset > 1e-6 {
                        let factor = limit / offset;
                        source[slot][0] = x as f32 + 0.5 + offset_x * factor;
                        source[slot][1] = y as f32 + 0.5 + offset_y * factor;
                    }
                }
            }
        }
        for y in top..=bottom {
            for x in left..=right {
                let slot = (y - top) * box_width + (x - left);
                let [sx, sy] = source[slot];
                if (sx - (x as f32 + 0.5)).abs() < 1e-4 && (sy - (y as f32 + 0.5)).abs() < 1e-4 {
                    continue;
                }
                let sampled = sample_box_bilinear(
                    &snapshot,
                    box_width,
                    box_height,
                    sx - left as f32,
                    sy - top as f32,
                );
                let base = (y * width + x) * 4;
                image.data[base..base + 4].copy_from_slice(&sampled);
            }
        }
    }
}

fn sample_box_bilinear(snapshot: &[f32], width: usize, height: usize, x: f32, y: f32) -> [f32; 4] {
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
        for channel in 0..4 {
            out[channel] += snapshot[base + channel] * weight;
        }
    }
    out
}

/// Apply clone/heal stamps to the composite in place.
///
/// Both modes sample a snapshot taken before the layer so a long stroke cannot
/// feed its own output back into the sample; heal additionally matches the
/// copied patch to the mean colour of the ring around the destination.
fn apply_retouch(
    image: &mut LinearImage,
    params: &RetouchParams,
    source_width: u32,
    source_height: u32,
    layer_opacity: f32,
) {
    if image.width == 0 || image.height == 0 {
        return;
    }
    apply_liquify_strokes(
        image,
        &params.warp,
        &params.freeze,
        &params.freeze_grid,
        source_width,
        source_height,
        layer_opacity,
    );
    if params.spots.is_empty() {
        return;
    }
    let snapshot = image.clone();
    // Retouch runs on the full (uncropped, unrotated) frame.
    let scale_x = image.width as f32 / source_width.max(1) as f32;
    let scale_y = image.height as f32 / source_height.max(1) as f32;
    let shortest = source_width.min(source_height) as f32;
    let width = image.width as usize;
    let height = image.height as usize;
    let to_view = |x: f32, y: f32| {
        (
            x * source_width as f32 * scale_x,
            y * source_height as f32 * scale_y,
        )
    };
    let sample_mean = |cx: f32, cy: f32, radius: f32, mirrored: bool| -> [f32; 4] {
        let mut sum = [0.0f32; 4];
        let mut count = 0.0f32;
        let inner = radius * 1.05;
        let outer = radius * 1.35;
        let left = (cx - outer).floor().max(0.0) as usize;
        let right = (cx + outer).ceil().min(width as f32 - 1.0).max(0.0) as usize;
        let top = (cy - outer).floor().max(0.0) as usize;
        let bottom = (cy + outer).ceil().min(height as f32 - 1.0).max(0.0) as usize;
        for y in top..=bottom {
            for x in left..=right {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance < inner || distance > outer {
                    continue;
                }
                let (sx, sy) = if mirrored {
                    (2.0 * cx - (x as f32 + 0.5), 2.0 * cy - (y as f32 + 0.5))
                } else {
                    (x as f32 + 0.5, y as f32 + 0.5)
                };
                if sx < 0.0 || sy < 0.0 || sx >= width as f32 || sy >= height as f32 {
                    continue;
                }
                let index = ((sy as usize) * width + sx as usize) * 4;
                for (channel, value) in sum.iter_mut().enumerate() {
                    *value += snapshot.data[index + channel];
                }
                count += 1.0;
            }
        }
        if count <= 0.0 {
            return [0.0; 4];
        }
        [
            sum[0] / count,
            sum[1] / count,
            sum[2] / count,
            sum[3] / count,
        ]
    };

    for spot in &params.spots {
        let (cx, cy) = to_view(spot.x, spot.y);
        let radius = (spot.radius * shortest * scale_x).max(1.0);
        let source = match (spot.mode, spot.source_x, spot.source_y) {
            (RetouchMode::Clone, Some(x), Some(y)) => Some(to_view(x, y)),
            _ => None,
        };
        // Clone copies the sample verbatim; heal matches the mirrored patch to
        // the mean colour of the ring around the destination.
        let shift = if spot.mode == RetouchMode::Heal {
            let dest = sample_mean(cx, cy, radius, false);
            let source = sample_mean(cx, cy, radius, true);
            [
                dest[0] - source[0],
                dest[1] - source[1],
                dest[2] - source[2],
                0.0,
            ]
        } else {
            [0.0; 4]
        };
        let inner = radius * spot.hardness;
        let opacity = (spot.opacity * layer_opacity).clamp(0.0, 1.0);
        let left = (cx - radius).floor().max(0.0) as usize;
        let right = (cx + radius).ceil().min(width as f32 - 1.0).max(0.0) as usize;
        let top = (cy - radius).floor().max(0.0) as usize;
        let bottom = (cy + radius).ceil().min(height as f32 - 1.0).max(0.0) as usize;
        for y in top..=bottom {
            for x in left..=right {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                if distance > radius {
                    continue;
                }
                let weight = (1.0 - smoothstep(inner, radius, distance)) * opacity;
                if weight <= 0.0 {
                    continue;
                }
                let (sx, sy) = match (spot.mode, source) {
                    (RetouchMode::Clone, Some((scx, scy))) => (scx + (px - cx), scy + (py - cy)),
                    _ => (2.0 * cx - px, 2.0 * cy - py),
                };
                if sx < 0.0 || sy < 0.0 || sx >= width as f32 || sy >= height as f32 {
                    continue;
                }
                let source_index = ((sy as usize) * width + sx as usize) * 4;
                let dest_index = (y * width + x) * 4;
                for (channel, shift_value) in shift.iter().enumerate() {
                    let sampled = snapshot.data[source_index + channel] + shift_value;
                    let current_value = image.data[dest_index + channel];
                    image.data[dest_index + channel] =
                        current_value + (sampled - current_value) * weight;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::{
        BlendMode as DocBlendMode, ColorParams, CropRect, Layer, LayerKind, LayerMask,
        MaskGradient, MaskGradientKind, MaskPoint, MaskStroke, ProjectDocument, SourceAsset,
    };
    use image_core::srgb_u8_to_linear;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Default)]
    struct StaticResolver {
        assets: HashMap<Uuid, DecodedImage>,
        luts: HashMap<Uuid, Lut>,
        rasters: HashMap<Uuid, Arc<RasterCoverage>>,
    }

    impl AssetResolver for StaticResolver {
        fn load_ai_result(&self, asset_id: Uuid) -> Result<DecodedImage, RenderError> {
            self.assets
                .get(&asset_id)
                .cloned()
                .ok_or(RenderError::MissingAsset {
                    asset_id: asset_id.to_string(),
                })
        }

        fn load_lut(&self, asset_id: Uuid) -> Result<Lut, RenderError> {
            self.luts
                .get(&asset_id)
                .cloned()
                .ok_or_else(|| RenderError::LutLoad {
                    asset_id: asset_id.to_string(),
                    message: "lut asset is not registered".into(),
                })
        }

        fn load_raster_mask(&self, asset_id: Uuid) -> Option<Arc<RasterCoverage>> {
            self.rasters.get(&asset_id).cloned()
        }
    }

    fn source() -> DecodedImage {
        let mut rgba = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                rgba.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, 32, 255]);
            }
        }
        DecodedImage::new(16, 16, rgba).unwrap()
    }

    fn doc_with_base() -> (ProjectDocument, Uuid) {
        let mut doc = ProjectDocument::new("渲染测试", 16, 16);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "source.png".into(),
            width: 16,
            height: 16,
            format: "png".into(),
            original_path: PathBuf::from("source.png").to_string_lossy().to_string(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        let base = Layer::base("背景", None);
        let base_id = base.id;
        doc.layers.push(base);
        (doc, base_id)
    }

    fn approx_eq(a: &LinearImage, b: &LinearImage, tolerance: f32) {
        assert_eq!(a.width, b.width);
        assert_eq!(a.height, b.height);
        for (x, y) in a.data.iter().zip(b.data.iter()) {
            assert!((x - y).abs() <= tolerance, "{x} vs {y}");
        }
    }

    #[test]
    fn base_only_matches_source() {
        let (doc, _) = doc_with_base();
        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        approx_eq(&rendered, &src.to_linear(), 0.0);
    }

    #[test]
    fn opacity_zero_layer_leaves_base_unchanged() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.opacity = 0.0;
        layer.color = Some(ColorParams {
            exposure: 2.5,
            ..Default::default()
        });
        doc.layers.push(layer);
        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        approx_eq(&rendered, &src.to_linear(), 0.0);
    }

    #[test]
    fn hidden_layer_has_no_effect() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.visible = false;
        layer.color = Some(ColorParams {
            exposure: 2.0,
            ..Default::default()
        });
        let layer_id = layer.id;
        doc.layers.push(layer);
        assert_eq!(
            doc.layers.iter().find(|l| l.id == layer_id).unwrap().kind,
            LayerKind::Color
        );
        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        approx_eq(&rendered, &src.to_linear(), 0.0);
    }

    #[test]
    fn each_layer_adjusts_the_composite_not_the_previous_layers_scratch() {
        // The renderer reuses one scratch buffer across layers. If it were not
        // reset per layer, the second layer would adjust the *first* layer's
        // adjusted pixels instead of the composite, and +3 EV followed by -3 EV
        // at full opacity would collapse back onto the untouched source.
        let (mut doc, _) = doc_with_base();
        let mut lift = Layer::color();
        lift.opacity = 0.5;
        lift.color = Some(ColorParams {
            exposure: 3.0,
            ..Default::default()
        });
        let mut drop = Layer::color();
        drop.color = Some(ColorParams {
            exposure: -3.0,
            ..Default::default()
        });
        doc.layers.push(lift.clone());
        doc.layers.push(drop.clone());

        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();

        let mut expected = src.to_linear();
        let mut lifted = expected.clone();
        apply_color_adjustments(&mut lifted, &lift.color.unwrap());
        composite_over(
            &mut expected,
            &lifted,
            lift.opacity,
            map_blend(lift.blend_mode),
        )
        .unwrap();
        let mut lowered = expected.clone();
        apply_color_adjustments(&mut lowered, &drop.color.unwrap());
        composite_over(
            &mut expected,
            &lowered,
            drop.opacity,
            map_blend(drop.blend_mode),
        )
        .unwrap();

        approx_eq(&rendered, &expected, 1e-5);
        let mut largest = 0.0f32;
        for (a, b) in rendered.data.iter().zip(src.to_linear().data.iter()) {
            largest = largest.max((a - b).abs());
        }
        assert!(
            largest > 0.01,
            "the reference must differ from the source or the assertion proves nothing"
        );
    }

    #[test]
    fn clone_stamp_copies_the_sampled_area_and_spares_the_rest() {
        let mut rgba = Vec::new();
        for _y in 0..16u32 {
            for x in 0..16u32 {
                let color = if x < 8 {
                    [200u8, 40, 40]
                } else {
                    [40, 200, 40]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(16, 16, rgba).unwrap();
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::retouch();
        if let Some(retouch) = &mut layer.retouch {
            retouch.spots.push(document::RetouchSpot {
                mode: document::RetouchMode::Clone,
                x: 0.75,
                y: 0.5,
                radius: 0.08,
                source_x: Some(0.25),
                source_y: Some(0.5),
                hardness: 1.0,
                opacity: 1.0,
            });
        }
        doc.layers.push(layer);

        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let reference = src.to_linear();
        let at = |image: &LinearImage, nx: f32, ny: f32| {
            let x = (nx * image.width as f32) as usize;
            let y = (ny * image.height as f32) as usize;
            let index = (y * image.width as usize + x) * 4;
            [
                image.data[index],
                image.data[index + 1],
                image.data[index + 2],
            ]
        };
        let stamped = at(&rendered, 0.75, 0.5);
        let expected = at(&reference, 0.25, 0.5);
        assert!(
            stamped[0] > stamped[1],
            "the red half was copied in: {stamped:?} expected {expected:?}"
        );
        for channel in 0..3 {
            assert!(
                (stamped[channel] - expected[channel]).abs() < 1e-4,
                "clone must copy the sample: {stamped:?} vs {expected:?}"
            );
        }
        let untouched = at(&rendered, 0.95, 0.1);
        let original = at(&reference, 0.95, 0.1);
        for channel in 0..3 {
            assert!((untouched[channel] - original[channel]).abs() < 1e-4);
        }
    }

    #[test]
    fn straighten_rotates_the_frame_and_leaves_transparent_corners() {
        let (mut doc, _) = doc_with_base();
        doc.crop = document::CropRect {
            x: 0,
            y: 0,
            width: 23,
            height: 23,
            angle: 45.0,
        };
        let rendered = render_document(&doc, &source(), None, &StaticResolver::default()).unwrap();
        assert_eq!((rendered.width, rendered.height), (23, 23));
        let alpha_at = |x: u32, y: u32| {
            let index = (y as usize * rendered.width as usize + x as usize) * 4 + 3;
            rendered.data[index]
        };
        assert!(
            alpha_at(0, 0) < 0.01,
            "the corner is outside the rotated frame"
        );
        assert!(alpha_at(22, 0) < 0.01);
        assert!(alpha_at(0, 22) < 0.01);
        assert!(alpha_at(22, 22) < 0.01);
        assert!(alpha_at(11, 11) > 0.99, "the centre stays opaque");
        // Every RGB value must stay finite through the bilinear sampling.
        assert!(rendered.data.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn straighten_crops_inside_the_rotated_frame() {
        let (mut doc, _) = doc_with_base();
        doc.crop = document::CropRect {
            x: 3,
            y: 4,
            width: 10,
            height: 6,
            angle: 10.0,
        };
        let rendered = render_document(&doc, &source(), None, &StaticResolver::default()).unwrap();
        assert_eq!((rendered.width, rendered.height), (10, 6));
    }

    #[test]
    fn retouch_coordinates_ignore_the_crop_offset() {
        // Full-frame mapping bug guard: the spot sits at 75% of the frame while
        // only the right half is exported; without ignoring the crop offset the
        // stamp would land outside the visible area.
        let mut rgba = Vec::new();
        for _y in 0..16u32 {
            for x in 0..16u32 {
                let color = if x < 8 {
                    [200u8, 20, 20]
                } else {
                    [20, 200, 20]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(16, 16, rgba).unwrap();
        let (mut doc, _) = doc_with_base();
        doc.crop = document::CropRect {
            x: 8,
            y: 0,
            width: 8,
            height: 16,
            angle: 0.0,
        };
        let mut layer = Layer::retouch();
        if let Some(retouch) = &mut layer.retouch {
            retouch.spots.push(document::RetouchSpot {
                mode: document::RetouchMode::Clone,
                x: 0.75,
                y: 0.5,
                radius: 0.08,
                source_x: Some(0.25),
                source_y: Some(0.5),
                hardness: 1.0,
                opacity: 1.0,
            });
        }
        doc.layers.push(layer);
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        assert_eq!((rendered.width, rendered.height), (8, 16));
        let pixel = |x: usize, y: usize| {
            let index = (y * rendered.width as usize + x) * 4;
            [rendered.data[index], rendered.data[index + 1]]
        };
        // 75% of 16 = 12, which is x=4 inside the right-half crop.
        let stamped = pixel(4, 8);
        assert!(
            stamped[0] > stamped[1],
            "the red sample was copied: {stamped:?}"
        );
        let untouched = pixel(6, 8);
        assert!(untouched[1] > untouched[0], "the rest stays green");
    }

    #[test]
    fn liquify_push_stroke_moves_the_boundary() {
        let width = 32u32;
        let height = 32u32;
        let mut rgba = Vec::new();
        for _y in 0..height {
            for x in 0..width {
                let color = if x < width / 2 {
                    [210u8, 30, 30]
                } else {
                    [30, 200, 40]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(width, height, rgba).unwrap();
        let mut doc = ProjectDocument::new("液化", width, height);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "liquify.png".into(),
            width,
            height,
            format: "png".into(),
            original_path: "liquify.png".into(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        doc.layers.push(Layer::base("背景", None));
        let mut layer = Layer::retouch();
        if let Some(retouch) = &mut layer.retouch {
            retouch.warp.push(document::LiquifyStroke {
                points: (0..9)
                    .map(|step| document::MaskPoint {
                        x: 0.25 + step as f32 * 0.0625,
                        y: 0.5,
                    })
                    .collect(),
                radius: 0.28,
                strength: 1.0,
                mode: document::LiquifyMode::Push,
            });
        }
        doc.layers.push(layer);

        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let reference = src.to_linear();
        let boundary = |image: &LinearImage| {
            (0..image.width)
                .find(|x| {
                    let pixel = image.pixel(*x, 16).unwrap();
                    pixel[1] > pixel[0]
                })
                .unwrap_or(image.width)
        };
        let before = boundary(&reference);
        let after = boundary(&rendered);
        assert!(
            after > before,
            "a rightward push should move the boundary right: {before} -> {after}"
        );
        // Pixels far below the stroke are untouched.
        let corner = rendered.pixel(1, 30).unwrap();
        let original = reference.pixel(1, 30).unwrap();
        assert!((corner[0] - original[0]).abs() < 1e-4);
    }

    #[test]
    fn freeze_stroke_protects_pixels_from_liquify() {
        let width = 32u32;
        let height = 32u32;
        let mut rgba = Vec::new();
        for _y in 0..height {
            for x in 0..width {
                let color = if x < width / 2 {
                    [210u8, 30, 30]
                } else {
                    [30, 200, 40]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(width, height, rgba).unwrap();
        let mut doc = ProjectDocument::new("冻结保护", width, height);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "freeze.png".into(),
            width,
            height,
            format: "png".into(),
            original_path: "freeze.png".into(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        doc.layers.push(Layer::base("背景", None));
        let mut layer = Layer::retouch();
        if let Some(retouch) = &mut layer.retouch {
            retouch.warp.push(document::LiquifyStroke {
                points: (0..9)
                    .map(|step| document::MaskPoint {
                        x: 0.25 + step as f32 * 0.0625,
                        y: 0.5,
                    })
                    .collect(),
                radius: 0.28,
                strength: 1.0,
                mode: document::LiquifyMode::Push,
            });
            retouch.freeze.push(document::MaskStroke {
                points: vec![
                    document::MaskPoint { x: 0.2, y: 0.5 },
                    document::MaskPoint { x: 0.8, y: 0.5 },
                ],
                radius: 0.5,
                feather: 0.0,
                erase: false,
            });
        }
        doc.layers.push(layer);

        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let reference = src.to_linear();
        let boundary = |image: &LinearImage| {
            (0..image.width)
                .find(|x| {
                    let pixel = image.pixel(*x, 16).unwrap();
                    pixel[1] > pixel[0]
                })
                .unwrap_or(image.width)
        };
        assert_eq!(
            boundary(&rendered),
            boundary(&reference),
            "a fully frozen stroke must cancel the warp"
        );
    }

    #[test]
    fn heal_stamp_changes_only_the_disc() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::retouch();
        if let Some(retouch) = &mut layer.retouch {
            retouch.spots.push(document::RetouchSpot {
                mode: document::RetouchMode::Heal,
                x: 0.5,
                y: 0.5,
                radius: 0.1,
                source_x: None,
                source_y: None,
                hardness: 0.0,
                opacity: 1.0,
            });
        }
        doc.layers.push(layer);
        let src = source();
        let reference = src.to_linear();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        assert!(rendered.data.iter().all(|value| value.is_finite()));
        let at = |image: &LinearImage, x: usize, y: usize| {
            let index = (y * image.width as usize + x) * 4;
            image.data[index]
        };
        assert!((at(&rendered, 8, 8) - at(&reference, 8, 8)).abs() > 1e-5);
        assert!((at(&rendered, 1, 1) - at(&reference, 1, 1)).abs() < 1e-6);
    }

    #[test]
    fn ai_result_missing_asset_is_typed_error() {
        let (mut doc, _) = doc_with_base();
        doc.layers.push(Layer::ai_result(Uuid::new_v4()));
        let err = render_document(&doc, &source(), None, &StaticResolver::default()).unwrap_err();
        assert_eq!(err.code(), "ASSET_MISSING");
    }

    #[test]
    fn ai_result_is_composited() {
        let (mut doc, _) = doc_with_base();
        let asset_id = Uuid::new_v4();
        let pixel = DecodedImage::new(4, 4, [200u8, 40, 20, 255].repeat(16)).unwrap();
        let mut resolver = StaticResolver::default();
        resolver.assets.insert(asset_id, pixel);
        let mut layer = Layer::ai_result(asset_id);
        layer.opacity = 1.0;
        layer.blend_mode = DocBlendMode::Normal;
        doc.layers.push(layer);
        let rendered = render_document(&doc, &source(), None, &resolver).unwrap();
        let center = rendered.pixel(8, 8).unwrap();
        assert!(center[0] > 0.4);
        assert!(center[0] > center[1]);
        assert!(center[2] < 0.1);
    }

    #[test]
    fn ai_result_with_another_ratio_is_cover_cropped_not_stretched() {
        let (mut doc, _) = doc_with_base();
        let asset_id = Uuid::new_v4();
        let mut rgba = Vec::new();
        for _y in 0..8u32 {
            for x in 0..32u32 {
                rgba.extend_from_slice(&[(x * 8) as u8, 0, 0, 255]);
            }
        }
        let asset = DecodedImage::new(32, 8, rgba).unwrap();
        let mut resolver = StaticResolver::default();
        resolver.assets.insert(asset_id, asset);
        let mut layer = Layer::ai_result(asset_id);
        layer.opacity = 1.0;
        doc.layers.push(layer);

        let rendered = render_document(&doc, &source(), None, &resolver).unwrap();
        assert_eq!((rendered.width, rendered.height), (16, 16));
        let left = rendered.pixel(0, 8).unwrap();
        let right = rendered.pixel(15, 8).unwrap();
        // Stretching would give R=0 at the left edge and R=248 at the right.
        // Cover-crop samples the center columns instead.
        assert!((left[0] - srgb_u8_to_linear(98)).abs() < 0.02);
        assert!((right[0] - srgb_u8_to_linear(158)).abs() < 0.02);
    }

    #[test]
    fn crop_is_applied_after_compositing_and_keeps_preview_size() {
        let (mut doc, _) = doc_with_base();
        doc.set_crop(CropRect {
            x: 4,
            y: 2,
            width: 8,
            height: 12,
            angle: 0.0,
        });
        let src = source();

        let full = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        assert_eq!((full.width, full.height), (8, 12));

        let preview = render_document(&doc, &src, Some(8), &StaticResolver::default()).unwrap();
        assert!(preview.width <= 8);
        assert!(preview.height <= 8);
        assert!(preview.width > 0 && preview.height > 0);
    }

    #[test]
    fn manual_mask_limits_a_color_layer_to_painted_pixels() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 2.0,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            strokes: vec![MaskStroke {
                points: vec![MaskPoint { x: 0.25, y: 0.5 }],
                radius: 0.25,
                feather: 0.0,
                erase: false,
            }],
            auto_skin: false,
            ..LayerMask::default()
        });
        doc.layers.push(layer);
        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let original = src.to_linear();
        let left = rendered.pixel(6, 8).unwrap();
        let left_original = original.pixel(6, 8).unwrap();
        let right = rendered.pixel(14, 8).unwrap();
        let right_original = original.pixel(14, 8).unwrap();
        assert!(left[0] > left_original[0] + 0.05);
        assert_eq!(right, right_original);
    }

    #[test]
    fn gradient_mask_selects_one_side_and_supports_invert_and_density() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 2.0,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            gradient: Some(MaskGradient {
                kind: MaskGradientKind::Linear,
                start: MaskPoint { x: 0.0, y: 0.5 },
                end: MaskPoint { x: 0.7, y: 0.5 },
            }),
            ..LayerMask::default()
        });
        let layer_id = layer.id;
        doc.layers.push(layer);
        let src = source();
        let resolver = StaticResolver::default();
        let rendered = render_document(&doc, &src, None, &resolver).unwrap();
        let original = src.to_linear();
        let left = rendered.pixel(2, 8).unwrap();
        let left_original = original.pixel(2, 8).unwrap();
        let right = rendered.pixel(14, 8).unwrap();
        let right_original = original.pixel(14, 8).unwrap();
        assert!(left[1] > left_original[1] + 0.05);
        assert!((right[1] - right_original[1]).abs() < 1e-4);

        let mask = doc.layer_mut(layer_id).unwrap().mask.as_mut().unwrap();
        mask.inverted = true;
        let inverted = render_document(&doc, &src, None, &resolver).unwrap();
        let left_inverted = inverted.pixel(2, 8).unwrap();
        let right_inverted = inverted.pixel(14, 8).unwrap();
        assert!((left_inverted[1] - left_original[1]).abs() < 0.1);
        assert!(right_inverted[1] > right_original[1] + 0.05);

        let mask = doc.layer_mut(layer_id).unwrap().mask.as_mut().unwrap();
        mask.inverted = false;
        mask.density = 0.0;
        let empty = render_document(&doc, &src, None, &resolver).unwrap();
        assert_eq!(empty.pixel(14, 8).unwrap(), right_original);
        let left_empty = empty.pixel(2, 8).unwrap();
        assert!((left_empty[1] - left_original[1]).abs() < 1e-5);
    }

    #[test]
    fn auto_skin_mask_limits_adjustments_to_skin_tones() {
        let mut rgba = Vec::new();
        for _y in 0..16u32 {
            for x in 0..16u32 {
                let pixel = if x < 8 {
                    [220, 160, 130, 255]
                } else {
                    [40, 80, 200, 255]
                };
                rgba.extend_from_slice(&pixel);
            }
        }
        let src = DecodedImage::new(16, 16, rgba).unwrap();
        let (mut doc, _) = doc_with_base();
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "skin.png".into(),
            width: 16,
            height: 16,
            format: "png".into(),
            original_path: "skin.png".into(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 1.5,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            strokes: Vec::new(),
            auto_skin: true,
            ..LayerMask::default()
        });
        doc.layers.push(layer);
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let original = src.to_linear();
        let skin = rendered.pixel(3, 8).unwrap();
        let skin_original = original.pixel(3, 8).unwrap();
        let background = rendered.pixel(13, 8).unwrap();
        let background_original = original.pixel(13, 8).unwrap();
        assert!(skin[0] > skin_original[0] + 0.05);
        assert_eq!(background, background_original);
    }

    #[test]
    fn cube_lut_is_applied_by_colour_layer() {
        let (mut doc, _) = doc_with_base();
        let asset_id = Uuid::new_v4();
        let cube = b"LUT_1D_SIZE 3\n0 0 0\n0.9 0.5 0.2\n1 1 1\n";
        let mut resolver = StaticResolver::default();
        resolver
            .luts
            .insert(asset_id, color_pipeline::parse_cube(cube).unwrap());
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            lut_asset_id: Some(asset_id),
            ..Default::default()
        });
        doc.layers.push(layer);
        let src = source();
        let rendered = render_document(&doc, &src, None, &resolver).unwrap();
        let before = src.to_linear();
        let (original_r, original_g, _) = {
            let px = before.pixel(8, 8).unwrap();
            (px[0], px[1], px[2])
        };
        let after = rendered.pixel(8, 8).unwrap();
        assert!(after[0] > original_r + 0.05);
        assert!((after[1] - original_g).abs() < 0.02);
    }

    #[test]
    fn zero_strength_lut_skips_asset_lookup() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            lut_asset_id: Some(Uuid::new_v4()),
            lut_strength: 0.0,
            ..Default::default()
        });
        doc.layers.push(layer);
        let src = source();
        // An unregistered asset id would raise LUT_INVALID if the renderer
        // tried to load it, so a bitwise-identical render proves the lookup was
        // skipped and the neutral colour maths short-circuited.
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        approx_eq(&rendered, &src.to_linear(), 0.0);
    }

    #[test]
    fn missing_lut_asset_is_typed_error() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            lut_asset_id: Some(Uuid::new_v4()),
            ..Default::default()
        });
        doc.layers.push(layer);
        let err = render_document(&doc, &source(), None, &StaticResolver::default()).unwrap_err();
        assert_eq!(err.code(), "LUT_INVALID");
    }

    #[test]
    fn region_mask_limits_adjustments_to_an_ellipse() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 1.5,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            strokes: Vec::new(),
            auto_skin: false,
            regions: vec![document::MaskRegion {
                kind: "leftEye".into(),
                center_x: 0.25,
                center_y: 0.5,
                radius_x: 0.18,
                radius_y: 0.3,
                angle: 0.0,
                feather: 0.2,
                strength: 1.0,
            }],
            ..LayerMask::default()
        });
        doc.layers.push(layer);
        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let original = src.to_linear();
        let inside = rendered.pixel(4, 8).unwrap();
        let inside_original = original.pixel(4, 8).unwrap();
        let outside = rendered.pixel(14, 8).unwrap();
        let outside_original = original.pixel(14, 8).unwrap();
        assert!(inside[0] > inside_original[0] * 1.5, "{inside:?}");
        assert_eq!(outside, outside_original);
    }

    #[test]
    fn portrait_fine_repair_uses_the_layer_geometry() {
        let width = 120u32;
        let height = 140u32;
        let mut rgba = Vec::new();
        for y in 0..height {
            for _x in 0..width {
                let color = if (56..66).contains(&y) {
                    [150u8, 120, 120]
                } else {
                    [208, 168, 140]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(width, height, rgba).unwrap();
        let mut doc = ProjectDocument::new("人像精细修复", width, height);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "face.png".into(),
            width,
            height,
            format: "png".into(),
            original_path: "face.png".into(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        doc.layers.push(Layer::base("背景", None));
        let ellipse = |cx: f32, cy: f32, rx: f32, ry: f32| document::PortraitEllipse {
            center_x: cx,
            center_y: cy,
            radius_x: rx,
            radius_y: ry,
            angle: 0.0,
            feather: 0.2,
        };
        let geometry = document::PortraitGeometry {
            face: Some(ellipse(0.5, 0.5, 0.35, 0.42)),
            left_eye: Some(ellipse(0.4, 0.42, 0.05, 0.025)),
            right_eye: Some(ellipse(0.6, 0.42, 0.05, 0.025)),
            ..Default::default()
        };
        let mut layer = Layer::portrait();
        layer.portrait = Some(document::PortraitParams {
            dark_circle: 100.0,
            geometry: Some(geometry),
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            regions: vec![document::MaskRegion {
                kind: "face".into(),
                center_x: 0.5,
                center_y: 0.5,
                radius_x: 0.4,
                radius_y: 0.45,
                angle: 0.0,
                feather: 0.2,
                strength: 1.0,
            }],
            ..LayerMask::default()
        });
        doc.layers.push(layer.clone());
        let repaired = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();

        // The same layer without geometry must leave the shadow untouched.
        let mut bare = layer;
        if let Some(params) = &mut bare.portrait {
            params.geometry = None;
        }
        doc.layers.pop();
        doc.layers.push(bare);
        let untouched = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();

        let at = |image: &LinearImage, x: u32, y: u32| {
            let pixel = image.pixel(x, y).unwrap();
            0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]
        };
        let before = at(&untouched, 48, 60);
        let after = at(&repaired, 48, 60);
        assert!(
            after > before * 1.08,
            "the measured under-eye region should lift the shadow: {before} -> {after}"
        );
    }

    #[test]
    fn region_mask_stays_aligned_when_the_export_is_cropped() {
        // Layers composite on the full frame, so a mask at 25% of the source
        // must land at 25% of the full frame even when only the left half is
        // exported. Subtracting the crop offset here would move it off-canvas.
        let mut rgba = Vec::new();
        for _y in 0..16u32 {
            for _x in 0..16u32 {
                rgba.extend_from_slice(&[120, 120, 120, 255]);
            }
        }
        let src = DecodedImage::new(16, 16, rgba).unwrap();
        let (mut doc, _) = doc_with_base();
        doc.crop = document::CropRect {
            x: 0,
            y: 0,
            width: 8,
            height: 16,
            angle: 0.0,
        };
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 2.0,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            regions: vec![document::MaskRegion {
                kind: "face".into(),
                center_x: 0.25,
                center_y: 0.5,
                radius_x: 0.12,
                radius_y: 0.3,
                angle: 0.0,
                feather: 0.0,
                strength: 1.0,
            }],
            ..LayerMask::default()
        });
        doc.layers.push(layer);

        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        assert_eq!((rendered.width, rendered.height), (8, 16));
        let inside = rendered.pixel(4, 8).unwrap();
        let outside = rendered.pixel(7, 8).unwrap();
        assert!(
            inside[0] > outside[0] * 1.2,
            "the region should cover x=4, not the cropped edge: {inside:?} vs {outside:?}"
        );
    }

    #[test]
    fn rotated_region_respects_its_angle() {
        let (mut doc, _) = doc_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 1.5,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            regions: vec![document::MaskRegion {
                kind: "skin".into(),
                center_x: 0.5,
                center_y: 0.5,
                radius_x: 0.45,
                radius_y: 0.06,
                angle: std::f32::consts::FRAC_PI_2,
                feather: 0.0,
                strength: 1.0,
            }],
            ..LayerMask::default()
        });
        doc.layers.push(layer);
        let src = source();
        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let original = src.to_linear();
        // A tall rotated ellipse covers the vertical middle, not the horizontal.
        assert!(
            rendered.pixel(8, 5).unwrap()[0] > original.pixel(8, 5).unwrap()[0],
            "vertical point should fall inside the rotated region"
        );
        assert_eq!(rendered.pixel(2, 8).unwrap(), original.pixel(2, 8).unwrap());
    }

    #[test]
    fn reports_backend_labels() {
        assert_eq!(BackendKind::Cpu.as_str(), "cpu");
        assert_eq!(BackendKind::Wgpu.as_str(), "wgpu");
    }

    #[test]
    fn body_slim_narrows_only_the_masked_body_region() {
        let width = 120u32;
        let height = 160u32;
        let mut rgba = Vec::new();
        for _y in 0..height {
            for x in 0..width {
                let color = if (40..80).contains(&x) {
                    [235u8, 235, 235]
                } else {
                    [60, 70, 80]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(width, height, rgba).unwrap();
        let build = |slim: f32| {
            let mut doc = ProjectDocument::new("身体塑形", width, height);
            doc.source = Some(SourceAsset {
                asset_id: Uuid::new_v4(),
                display_name: "body.png".into(),
                width,
                height,
                format: "png".into(),
                original_path: "body.png".into(),
                file_bytes: 0,
                color_space: "sRGB".into(),
                icc_asset_id: None,
            });
            doc.layers.push(Layer::base("背景", None));
            let mut layer = Layer::portrait();
            layer.portrait = Some(document::PortraitParams {
                body_mode: true,
                body_slim: slim,
                geometry: Some(document::PortraitGeometry {
                    face: Some(document::PortraitEllipse {
                        center_x: 0.5,
                        center_y: 0.28,
                        radius_x: 0.18,
                        radius_y: 0.2,
                        angle: 0.0,
                        feather: 0.2,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            });
            layer.mask = Some(LayerMask {
                regions: vec![document::MaskRegion {
                    kind: "body".into(),
                    center_x: 0.5,
                    center_y: 0.72,
                    radius_x: 0.4,
                    radius_y: 0.22,
                    angle: 0.0,
                    feather: 0.15,
                    strength: 1.0,
                }],
                ..LayerMask::default()
            });
            doc.layers.push(layer);
            doc
        };
        let base = render_document(&build(0.0), &src, None, &StaticResolver::default()).unwrap();
        let warped =
            render_document(&build(100.0), &src, None, &StaticResolver::default()).unwrap();
        let row = 115u32;
        let stripe = |image: &LinearImage| {
            (0..width)
                .filter(|x| image.pixel(*x, row).unwrap()[0] > 0.5)
                .count()
        };
        let before = stripe(&base);
        let after = stripe(&warped);
        assert!(before > 30, "control stripe width {before}");
        assert!(
            after + 6 < before,
            "slimming should narrow the masked stripe: {before} -> {after}"
        );
        assert_eq!(
            warped.pixel(5, 155).unwrap(),
            base.pixel(5, 155).unwrap(),
            "pixels outside the mask must stay identical"
        );
    }

    #[test]
    fn freeze_grid_protects_only_marked_cells() {
        let width = 32u32;
        let height = 32u32;
        let mut rgba = Vec::new();
        for _y in 0..height {
            for x in 0..width {
                let color = if x < width / 2 {
                    [210u8, 30, 30]
                } else {
                    [30, 200, 40]
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        let src = DecodedImage::new(width, height, rgba).unwrap();
        let mut doc = ProjectDocument::new("网格保护", width, height);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "grid.png".into(),
            width,
            height,
            format: "png".into(),
            original_path: "grid.png".into(),
            file_bytes: 0,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        doc.layers.push(Layer::base("背景", None));
        let mut layer = Layer::retouch();
        if let Some(retouch) = &mut layer.retouch {
            retouch.warp.push(document::LiquifyStroke {
                points: (0..9)
                    .map(|step| document::MaskPoint {
                        x: 0.25 + step as f32 * 0.0625,
                        y: 0.5,
                    })
                    .collect(),
                radius: 0.28,
                strength: 1.0,
                mode: document::LiquifyMode::Push,
            });
            retouch.freeze_grid =
                vec![false; document::FREEZE_GRID_COLS * document::FREEZE_GRID_ROWS];
            for column in 0..document::FREEZE_GRID_COLS {
                retouch.freeze_grid[8 * document::FREEZE_GRID_COLS + column] = true;
            }
        }
        doc.layers.push(layer);

        let rendered = render_document(&doc, &src, None, &StaticResolver::default()).unwrap();
        let reference = src.to_linear();
        let boundary = |image: &LinearImage, row: u32| {
            (0..image.width)
                .find(|x| {
                    let pixel = image.pixel(*x, row).unwrap();
                    pixel[1] > pixel[0]
                })
                .unwrap_or(image.width)
        };
        assert_eq!(
            boundary(&rendered, 16),
            boundary(&reference, 16),
            "the protected row must not move"
        );
        assert!(
            boundary(&rendered, 22) > boundary(&reference, 22),
            "rows outside the protected cells still follow the stroke"
        );
    }

    #[test]
    fn blur_spreads_energy_and_leaves_alpha_alone() {
        let mut image = LinearImage::new(8, 8, vec![0.0f32; 8 * 8 * 4]).unwrap();
        for index in 0..8 * 8 {
            image.data[index * 4 + 3] = 1.0;
        }
        let centre = (3 * 8 + 3) * 4;
        image.data[centre] = 1.0;
        image.data[centre + 1] = 1.0;
        image.data[centre + 2] = 1.0;

        blur_linear_rgb(&mut image, 2);

        assert!(
            image.data[centre] < 0.9,
            "the centre must spread into its neighbours"
        );
        let neighbour = (3 * 8 + 2) * 4;
        assert!(
            image.data[neighbour] > 0.0,
            "neighbours must receive energy"
        );
        assert_eq!(image.data[centre + 3], 1.0, "alpha is untouched");
    }

    #[test]
    fn zero_radius_blur_is_a_noop() {
        let mut image = LinearImage::new(2, 2, vec![0.25f32; 2 * 2 * 4]).unwrap();
        let before = image.data.clone();
        blur_linear_rgb(&mut image, 0);
        assert_eq!(image.data, before);
    }

    #[test]
    fn raster_coverage_samples_bilinearly_and_clamps() {
        let coverage = RasterCoverage::from_values(2, 1, vec![0.0, 1.0]).unwrap();
        assert_eq!(coverage.sample(0.0, 0.5), 0.0);
        assert_eq!(coverage.sample(1.0, 0.5), 1.0);
        assert!((coverage.sample(0.5, 0.5) - 0.5).abs() < 1e-6);
        // Outside the mask clamps to the border instead of wrapping.
        assert_eq!(coverage.sample(-3.0, 0.5), 0.0);
        assert_eq!(coverage.sample(4.0, 0.5), 1.0);
    }

    #[test]
    fn raster_mask_merges_into_coverage_and_respects_threshold() {
        let asset = Uuid::new_v4();
        let mask = LayerMask {
            raster: Some(document::MaskRaster {
                asset_id: asset,
                threshold: 0.5,
                softness: 0.0,
                source: "test".into(),
            }),
            ..LayerMask::default()
        };
        let image = LinearImage::transparent(4, 4).unwrap();

        let mut full = StaticResolver::default();
        full.rasters.insert(
            asset,
            Arc::new(RasterCoverage::from_values(2, 2, vec![1.0; 4]).unwrap()),
        );
        let coverage = rasterize_mask(
            &mask,
            &image,
            4,
            4,
            4,
            4,
            full.load_raster_mask(asset).as_deref(),
        );
        assert!(
            coverage.iter().all(|value| (*value - 1.0).abs() < 1e-6),
            "a fully selected raster covers the whole frame"
        );

        // No registered coverage: the raster contributes nothing, so the layer
        // behaves as if it had no mask at all.
        let empty = StaticResolver::default();
        let coverage = rasterize_mask(
            &mask,
            &image,
            4,
            4,
            4,
            4,
            empty.load_raster_mask(asset).as_deref(),
        );
        assert!(coverage.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn raster_mask_is_combined_with_brush_strokes_by_maximum() {
        let asset = Uuid::new_v4();
        // Brush covers the top-left pixel only; the raster covers the half.
        let mask = LayerMask {
            strokes: vec![MaskStroke {
                points: vec![MaskPoint { x: 0.1, y: 0.1 }],
                radius: 0.05,
                feather: 0.2,
                erase: false,
            }],
            raster: Some(document::MaskRaster {
                asset_id: asset,
                threshold: 0.5,
                softness: 0.0,
                source: "test".into(),
            }),
            ..LayerMask::default()
        };
        let image = LinearImage::transparent(4, 4).unwrap();
        let mut resolver = StaticResolver::default();
        resolver.rasters.insert(
            asset,
            Arc::new(RasterCoverage::from_values(2, 1, vec![0.0, 1.0]).unwrap()),
        );
        let coverage = rasterize_mask(
            &mask,
            &image,
            4,
            4,
            4,
            4,
            resolver.load_raster_mask(asset).as_deref(),
        );
        // Brush keeps the left edge, raster keeps the right half.
        assert!(coverage[0] > 0.0, "brush coverage must survive");
        assert!(coverage[3] > 0.5, "raster coverage must be merged in");
    }
}
