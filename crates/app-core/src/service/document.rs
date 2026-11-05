use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use base64::Engine as _;
use color_pipeline::Lut;
use document::{CropRect, DocumentError, Layer, LayerKind, MoveDirection, ProjectDocument};
use image_core::{
    decode_image_from_memory, decode_image_from_path, encode_atomic, encode_atomic_image,
    probe_image_dimensions, DecodedImage, EncodeOptions, LinearImage, OutputFormat, RasterImage,
    WatermarkAnchor, WatermarkSpec,
};
use portrait_engine::regions::RegionKind;
use portrait_engine::{FaceAnalyzer, PortraitWarning};
use render_engine::{render_document, AssetResolver, RenderError};
use task_runtime::{CancellationToken, TaskPriority};
use uuid::Uuid;

use super::{
    decode_source_image, parse_uuid, AppService, DEFAULT_PREVIEW_MAX, MAX_CACHED_HISTOGRAMS,
    MAX_CACHED_LUTS, MAX_PREVIEW_DIMENSION, SOURCE_PREVIEW_MAX,
};
use crate::dto::{
    AutoToneResult, DocumentState, ExportBitDepth, ExportColorSpace, ExportImageFormat,
    ExportMetadata, ExportProgress, ExportResult, Histogram, PortraitAnalysisResult,
    PortraitFaceRegions, PortraitKeypoint, PortraitRegion, PortraitRegionsResult, PreviewResult,
    WatermarkOptions, WatermarkPosition, WhiteBalanceResult,
};
use crate::error::ApiError;
use crate::requests::{
    AddLayerKind, AddLayerRequest, CancelExportRequest, CommitChangesRequest, DeleteLayerRequest,
    DuplicateLayerRequest, ExportImageRequest, MoveLayerRequest, PickWhiteBalanceRequest,
    ProjectIdRequest, RenderPreviewRequest, SetCropRequest, SetLayerMaskRequest,
    UpdateLayerParamsRequest, UpdateLayerRequest,
};

/// Render previews are rewritten for every revision, so the cache directory is
/// pruned after each successful render. Two are kept: the one the canvas is
/// showing and the one a still-loading `<img>` may be pointing at.
const RENDER_PREVIEW_KEEP: usize = 2;
/// Watermark payloads arrive over IPC; bound both the encoded string and the
/// decoded pixels so a malformed request cannot exhaust memory.
const MAX_WATERMARK_BYTES: usize = 8 * 1024 * 1024;
const MAX_WATERMARK_DIMENSION: u32 = 4096;

/// Decode the UI-rasterized watermark and blend it into the finished raster.
fn apply_export_watermark(
    raster: &mut RasterImage,
    options: &WatermarkOptions,
) -> Result<(), ApiError> {
    let encoded = options
        .image_base64
        .rsplit(',')
        .next()
        .unwrap_or_default()
        .trim();
    if encoded.is_empty() {
        return Err(ApiError::invalid_argument(
            "watermark image is empty",
            "水印图片为空，请重新选择图片或输入文字。",
        ));
    }
    if encoded.len() > MAX_WATERMARK_BYTES * 2 {
        return Err(ApiError::invalid_argument(
            "watermark payload is too large",
            "水印图片过大（最大 8MB），请换一张更小的图片。",
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| {
            ApiError::invalid_argument(
                "watermark image is not valid base64",
                "水印图片无法读取，请重新选择。",
            )
        })?;
    if bytes.len() > MAX_WATERMARK_BYTES {
        return Err(ApiError::invalid_argument(
            "watermark image is too large",
            "水印图片过大（最大 8MB），请换一张更小的图片。",
        ));
    }
    let stamp = decode_image_from_memory(&bytes).map_err(|_| {
        ApiError::invalid_argument(
            "watermark image could not be decoded",
            "水印图片无法读取，请使用 PNG、JPG 或 WebP 图片。",
        )
    })?;
    if stamp.width > MAX_WATERMARK_DIMENSION || stamp.height > MAX_WATERMARK_DIMENSION {
        return Err(ApiError::invalid_argument(
            "watermark image dimensions are too large",
            "水印图片尺寸过大（最大 4096px），请缩小后再试。",
        ));
    }
    let anchor = match options.position {
        WatermarkPosition::TopLeft => WatermarkAnchor::TopLeft,
        WatermarkPosition::TopRight => WatermarkAnchor::TopRight,
        WatermarkPosition::BottomLeft => WatermarkAnchor::BottomLeft,
        WatermarkPosition::BottomRight => WatermarkAnchor::BottomRight,
        WatermarkPosition::Center => WatermarkAnchor::Center,
    };
    image_core::composite_watermark(
        raster,
        &WatermarkSpec {
            image: &stamp,
            opacity: options.opacity,
            scale: options.scale,
            margin: options.margin,
            anchor,
        },
    )
    .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(())
}

struct DbAssetResolver<'a> {
    db: &'a persistence::Database,
    luts: &'a std::sync::Mutex<std::collections::HashMap<Uuid, Lut>>,
}

impl AssetResolver for DbAssetResolver<'_> {
    fn load_ai_result(&self, asset_id: Uuid) -> Result<DecodedImage, RenderError> {
        let record = self
            .db
            .get_asset(asset_id)
            .map_err(|error| RenderError::AssetLoad {
                asset_id: asset_id.to_string(),
                message: error.to_string(),
            })?
            .ok_or_else(|| RenderError::MissingAsset {
                asset_id: asset_id.to_string(),
            })?;
        if record.kind != "ai_result" {
            return Err(RenderError::MissingAsset {
                asset_id: asset_id.to_string(),
            });
        }
        decode_image_from_path(Path::new(&record.path)).map_err(|error| RenderError::AssetLoad {
            asset_id: asset_id.to_string(),
            message: error.to_string(),
        })
    }

    fn load_lut(&self, asset_id: Uuid) -> Result<Lut, RenderError> {
        let failed = |message: String| RenderError::LutLoad {
            asset_id: asset_id.to_string(),
            message,
        };
        if let Ok(cache) = self.luts.lock() {
            if let Some(lut) = cache.get(&asset_id) {
                return Ok(lut.clone());
            }
        }
        let record = self
            .db
            .get_asset(asset_id)
            .map_err(|error| failed(error.to_string()))?
            .ok_or_else(|| failed("asset record is missing".into()))?;
        if record.kind != "lut" {
            return Err(failed("asset is not a LUT".into()));
        }
        let bytes =
            std::fs::read(Path::new(&record.path)).map_err(|error| failed(error.to_string()))?;
        let lut = color_pipeline::parse_cube(&bytes).map_err(failed)?;
        // Asset ids are minted per import, so an entry can never go stale; the
        // cap only bounds how many parsed cubes are held at once.
        if let Ok(mut cache) = self.luts.lock() {
            if cache.len() >= MAX_CACHED_LUTS {
                cache.clear();
            }
            cache.insert(asset_id, lut.clone());
        }
        Ok(lut)
    }
}

/// `leftEye`-style identifiers for the UI, matching `MaskRegion.kind`.
fn camel_kind(kind: RegionKind) -> String {
    let label = match kind {
        RegionKind::Skin => "skin",
        RegionKind::LeftEye => "leftEye",
        RegionKind::RightEye => "rightEye",
        RegionKind::LeftBrow => "leftBrow",
        RegionKind::RightBrow => "rightBrow",
        RegionKind::Mouth => "mouth",
        RegionKind::Lips => "lips",
        RegionKind::Teeth => "teeth",
        RegionKind::Nose => "nose",
    };
    label.to_string()
}

impl AppService {
    /// Histogram for an already rendered preview. The cache path encodes both
    /// revision and dimension, so an entry can never describe other pixels.
    fn preview_histogram_cached(&self, path: &Path) -> Option<Histogram> {
        if let Ok(cache) = self.histograms.lock() {
            if let Some(histogram) = cache.get(path) {
                return Some(histogram.clone());
            }
        }
        let histogram = decode_image_from_path(path)
            .ok()
            .map(|image| preview_histogram(&image))?;
        self.store_histogram(path, &histogram);
        Some(histogram)
    }

    fn store_histogram(&self, path: &Path, histogram: &Histogram) {
        if let Ok(mut cache) = self.histograms.lock() {
            if cache.len() >= MAX_CACHED_HISTOGRAMS {
                cache.clear();
            }
            cache.insert(path.to_path_buf(), histogram.clone());
        }
    }
}

const PREVIEW_HISTOGRAM_BINS: usize = 64;

fn preview_histogram(image: &DecodedImage) -> Histogram {
    let mut red = vec![0u32; PREVIEW_HISTOGRAM_BINS];
    let mut green = vec![0u32; PREVIEW_HISTOGRAM_BINS];
    let mut blue = vec![0u32; PREVIEW_HISTOGRAM_BINS];
    let mut luma = vec![0u32; PREVIEW_HISTOGRAM_BINS];
    for pixel in image.rgba8.chunks_exact(4) {
        let r = pixel[0] as usize;
        let g = pixel[1] as usize;
        let b = pixel[2] as usize;
        red[r * PREVIEW_HISTOGRAM_BINS / 256] += 1;
        green[g * PREVIEW_HISTOGRAM_BINS / 256] += 1;
        blue[b * PREVIEW_HISTOGRAM_BINS / 256] += 1;
        let value = (2126 * r + 7152 * g + 722 * b) / 10_000;
        luma[value * PREVIEW_HISTOGRAM_BINS / 256] += 1;
    }
    Histogram {
        red,
        green,
        blue,
        luma,
    }
}

fn source_luma_histogram(image: &DecodedImage) -> [u32; 256] {
    let mut histogram = [0u32; 256];
    let step = (image.pixel_count() / 250_000).max(1);
    for pixel in image.rgba8.chunks_exact(4).step_by(step) {
        let value = (2126 * pixel[0] as usize + 7152 * pixel[1] as usize + 722 * pixel[2] as usize)
            / 10_000;
        histogram[value.min(255)] += 1;
    }
    histogram
}

/// Map a pixel crop expressed in original-source coordinates onto the proxy, so
/// the same region is rendered after the proxy replaces the original. Keeping
/// this in one place makes the cached and freshly decoded proxy paths identical.
fn scale_crop_to_proxy(
    document: &ProjectDocument,
    proxy: &DecodedImage,
    asset_width: u32,
    asset_height: u32,
) -> ProjectDocument {
    let mut scaled = document.clone();
    if proxy.width != asset_width || proxy.height != asset_height {
        let sx = proxy.width as f64 / asset_width.max(1) as f64;
        let sy = proxy.height as f64 / asset_height.max(1) as f64;
        let next = &mut scaled.crop;
        next.x = (next.x as f64 * sx).round() as u32;
        next.y = (next.y as f64 * sy).round() as u32;
        if next.width > 0 {
            next.width = (next.width as f64 * sx).round().max(1.0) as u32;
        }
        if next.height > 0 {
            next.height = (next.height as f64 * sy).round().max(1.0) as u32;
        }
        next.x = next.x.min(proxy.width.saturating_sub(1));
        next.y = next.y.min(proxy.height.saturating_sub(1));
        if next.width > 0 {
            next.width = next.width.min(proxy.width - next.x).max(1);
        }
        if next.height > 0 {
            next.height = next.height.min(proxy.height - next.y).max(1);
        }
    }
    scaled
}

impl AppService {
    /// Use the on-disk 1600px source proxy for ordinary previews instead of
    /// decoding the original. Small crops still need the full source so their
    /// preview has enough pixels; 2048px previews keep using the original.
    fn preview_render_input(
        &self,
        project_id: Uuid,
        document: &ProjectDocument,
        max_dimension: u32,
    ) -> Result<(ProjectDocument, Arc<DecodedImage>), ApiError> {
        if max_dimension <= SOURCE_PREVIEW_MAX {
            if let Some(asset) = document.source.as_ref() {
                let crop = document.crop.clamped(asset.width, asset.height);
                let (frame_width, frame_height) =
                    CropRect::rotated_frame(asset.width, asset.height, crop.angle);
                let frame_long = frame_width.max(frame_height).max(1);
                let crop_long = (if crop.width == 0 {
                    frame_width
                } else {
                    crop.width
                })
                .max(if crop.height == 0 {
                    frame_height
                } else {
                    crop.height
                });
                let required = max_dimension as f64 / SOURCE_PREVIEW_MAX as f64;
                if crop_long as f64 / frame_long as f64 + 0.05 >= required {
                    // Decoding the 1600px PNG costs more than the preview render
                    // at this size, and the proxy cannot change while the
                    // project is open, so the decoded pixels are reused across
                    // frames instead of being decoded on every slider tick.
                    if let Some(proxy) = self.cached_source_proxy(project_id, asset.asset_id) {
                        let scaled =
                            scale_crop_to_proxy(document, &proxy, asset.width, asset.height);
                        return Ok((scaled, proxy));
                    }
                    let path = self.assets.source_preview_path(
                        project_id,
                        asset.asset_id,
                        SOURCE_PREVIEW_MAX,
                    );
                    if path.is_file() {
                        if let Ok(decoded) = decode_source_image(&path) {
                            let proxy = Arc::new(decoded);
                            let scaled =
                                scale_crop_to_proxy(document, &proxy, asset.width, asset.height);
                            self.store_source_proxy(project_id, asset.asset_id, proxy.clone());
                            return Ok((scaled, proxy));
                        }
                    }
                }
            }
        }
        let source = self.load_source_image(project_id, document)?;
        Ok((document.clone(), source))
    }

    pub fn render_preview(&self, request: RenderPreviewRequest) -> Result<PreviewResult, ApiError> {
        let started = Instant::now();
        let project_id = parse_uuid(&request.project_id, "project")?;
        let max_dimension = request.max_dimension.unwrap_or(DEFAULT_PREVIEW_MAX);
        if max_dimension == 0 || max_dimension > MAX_PREVIEW_DIMENSION {
            return Err(ApiError::invalid_argument(
                format!("invalid preview max dimension: {max_dimension}"),
                "预览尺寸无效。",
            ));
        }

        let mut document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        if request.ignore_crop {
            // The crop tool needs the whole frame; straightening is previewed
            // client-side until the crop is applied.
            if let Some(source) = &document.source {
                document.crop = CropRect::full(source.width, source.height);
            }
        }
        let cache_path = self.assets.render_preview_path_variant(
            project_id,
            document.revision,
            max_dimension,
            request.ignore_crop,
        );

        if cache_path.exists() {
            if let Ok((width, height)) = probe_image_dimensions(&cache_path) {
                return Ok(PreviewResult {
                    path: cache_path.to_string_lossy().to_string(),
                    width,
                    height,
                    revision: document.revision,
                    duration_ms: started.elapsed().as_millis() as u64,
                    cached: true,
                    backend: "cache".to_string(),
                    histogram: self.preview_histogram_cached(&cache_path),
                });
            }
        }

        let (preview_document, source) =
            self.preview_render_input(project_id, &document, max_dimension)?;
        let resolver = DbAssetResolver {
            db: &self.db,
            luts: &self.luts,
        };
        let (encoded, backend) = if render_engine::gpu_available() {
            match render_engine::render_document_gpu(
                &preview_document,
                &source,
                Some(max_dimension),
                &resolver,
            ) {
                Some(Ok(encoded)) => (encoded, "wgpu"),
                Some(Err(error)) => return Err(error.into()),
                None => {
                    let rendered = render_document(
                        &preview_document,
                        &source,
                        Some(max_dimension),
                        &resolver,
                    )?;
                    (rendered.to_srgb8(), "cpu")
                }
            }
        } else {
            let rendered =
                render_document(&preview_document, &source, Some(max_dimension), &resolver)?;
            (rendered.to_srgb8(), "cpu")
        };
        let histogram = preview_histogram(&encoded);
        self.store_histogram(&cache_path, &histogram);
        self.assets.ensure_project_dirs(project_id)?;
        encode_atomic(&cache_path, &encoded, OutputFormat::Png, 100)?;
        let _ = self.assets.prune_render_previews(
            project_id,
            RENDER_PREVIEW_KEEP,
            Some(document.revision),
        );
        Ok(PreviewResult {
            path: cache_path.to_string_lossy().to_string(),
            width: encoded.width,
            height: encoded.height,
            revision: document.revision,
            duration_ms: started.elapsed().as_millis() as u64,
            cached: false,
            backend: backend.to_string(),
            histogram: Some(histogram),
        })
    }

    pub fn update_layer_params(
        &self,
        request: UpdateLayerParamsRequest,
    ) -> Result<DocumentState, ApiError> {
        self.write_layer_params(request, true)
    }

    /// Same write as [`Self::update_layer_params`], but the response carries an
    /// empty `layers` array.
    ///
    /// Interactive drags only merge `revision` / history metadata; echoing every
    /// layer with its mask strokes and retouch points over IPC on each ~70 ms
    /// tick is pure overhead. Callers that apply the returned document must stay
    /// on the full variant.
    pub fn update_layer_params_meta(
        &self,
        request: UpdateLayerParamsRequest,
    ) -> Result<DocumentState, ApiError> {
        self.write_layer_params(request, false)
    }

    fn write_layer_params(
        &self,
        request: UpdateLayerParamsRequest,
        include_layers: bool,
    ) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        if request.color.is_none() && request.portrait.is_none() && request.retouch.is_none() {
            return Err(ApiError::invalid_argument(
                "no layer parameters supplied",
                "没有可应用的调整参数。",
            ));
        }
        self.session(project_id, |project| {
            let layer = project
                .session
                .document()
                .layer(layer_id)
                .ok_or_else(|| ApiError::from(DocumentError::LayerNotFound))?;
            if layer.kind == LayerKind::Base {
                return Err(ApiError::from(DocumentError::BaseLayerProtected));
            }
            if request.color.is_some() && layer.kind != LayerKind::Color {
                return Err(ApiError::invalid_argument(
                    "color params sent to a non-color layer",
                    "该图层不支持调色参数。",
                ));
            }
            if request.portrait.is_some() && layer.kind != LayerKind::Portrait {
                return Err(ApiError::invalid_argument(
                    "portrait params sent to a non-portrait layer",
                    "该图层不支持人像参数。",
                ));
            }
            if request.retouch.is_some() && layer.kind != LayerKind::Retouch {
                return Err(ApiError::invalid_argument(
                    "retouch params sent to a non-retouch layer",
                    "该图层不支持局部修复参数。",
                ));
            }
            let color = request.color;
            let portrait = request.portrait;
            let retouch = request.retouch;
            project.session.mutate(|document| {
                if let Some(layer) = document.layer_mut(layer_id) {
                    if let Some(color) = color {
                        layer.color = Some(color.sanitized());
                    }
                    if let Some(portrait) = portrait {
                        layer.portrait = Some(portrait.sanitized());
                    }
                    if let Some(retouch) = retouch {
                        layer.retouch = Some(retouch);
                    }
                }
            });
            // Deliberately no history entry and no autosave: sliders call this
            // continuously, then `commit_changes` once.
            Ok(if include_layers {
                self.document_state(project_id, &project.session)
            } else {
                self.document_meta(project_id, &project.session)
            })
        })
    }

    pub fn set_layer_mask(&self, request: SetLayerMaskRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        self.session(project_id, |project| {
            let layer = project
                .session
                .document()
                .layer(layer_id)
                .ok_or_else(|| ApiError::from(DocumentError::LayerNotFound))?;
            if layer.kind == LayerKind::Base {
                return Err(ApiError::from(DocumentError::BaseLayerProtected));
            }
            if !matches!(layer.kind, LayerKind::Color | LayerKind::Portrait) {
                return Err(ApiError::invalid_argument(
                    "layer does not support a manual mask",
                    "只有调色或人像图层支持局部画笔。",
                ));
            }
            let mask = request.mask;
            project.session.mutate(|document| {
                if let Some(layer) = document.layer_mut(layer_id) {
                    layer.mask = mask;
                    layer.sanitize();
                }
            });
            let stored_mask = project
                .session
                .document()
                .layer(layer_id)
                .and_then(|layer| layer.mask.as_ref());
            let auto_only = stored_mask.is_some_and(|mask| {
                mask.auto_skin && mask.strokes.is_empty() && mask.gradient.is_none()
            });
            let gradient_only = stored_mask.is_some_and(|mask| {
                mask.gradient.is_some() && mask.strokes.is_empty() && !mask.auto_skin
            });
            let has_mask = stored_mask.is_some();
            let has_gradient = stored_mask.is_some_and(|mask| mask.gradient.is_some());
            project.session.commit(if auto_only {
                "自动肤色范围"
            } else if gradient_only {
                "渐变修饰范围"
            } else if has_gradient && has_mask {
                "渐变与画笔修饰"
            } else if has_mask {
                "画笔修饰"
            } else {
                "清除修饰范围"
            });
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn commit_changes(&self, request: CommitChangesRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.session(project_id, |project| {
            // Discrete commands already commit and save. When the document
            // matches its baseline there is nothing new to persist, and
            // rewriting the whole JSON is the most expensive thing this call
            // does (tens of milliseconds for a mask-heavy document).
            if project.session.commit(&request.label) {
                project.request_save();
            }
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn set_crop(&self, request: SetCropRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.session(project_id, |project| {
            project.session.mutate(|document| {
                document.set_crop(request.crop);
            });
            project.session.commit("裁剪照片");
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn auto_tone(&self, request: ProjectIdRequest) -> Result<AutoToneResult, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        let source = self.load_analysis_image(project_id, &document)?;
        let histogram = source_luma_histogram(&source);
        let suggestion =
            color_pipeline::auto_tone_from_luma_histogram(&histogram).ok_or_else(|| {
                ApiError::invalid_argument(
                    "source image has no analyzable pixels",
                    "这张照片没有可分析的像素。",
                )
            })?;
        Ok(AutoToneResult {
            exposure: suggestion.exposure,
            contrast: suggestion.contrast,
            whites: suggestion.whites,
            blacks: suggestion.blacks,
        })
    }

    pub fn pick_white_balance(
        &self,
        request: PickWhiteBalanceRequest,
    ) -> Result<WhiteBalanceResult, ApiError> {
        if !request.x.is_finite()
            || !request.y.is_finite()
            || !(0.0..=1.0).contains(&request.x)
            || !(0.0..=1.0).contains(&request.y)
        {
            return Err(ApiError::invalid_argument(
                "white balance point must be normalized",
                "采样位置无效，请重新点击画面。",
            ));
        }
        let project_id = parse_uuid(&request.project_id, "project")?;
        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        let source = self.load_analysis_image(project_id, &document)?;
        let width = source.width as i64;
        let height = source.height as i64;
        if width == 0 || height == 0 {
            return Err(ApiError::invalid_argument(
                "source image has no pixels",
                "这张照片没有可采样的像素。",
            ));
        }
        let center_x = (request.x * (width - 1) as f32).round() as i64;
        let center_y = (request.y * (height - 1) as f32).round() as i64;
        let radius = ((width.min(height) as f32 / 500.0).round() as i64).clamp(2, 8);

        let mut sum = [0.0f64; 3];
        let mut count = 0u32;
        for y in (center_y - radius).max(0)..=(center_y + radius).min(height - 1) {
            for x in (center_x - radius).max(0)..=(center_x + radius).min(width - 1) {
                let offset = ((y * width + x) * 4) as usize;
                let pixel = &source.rgba8[offset..offset + 3];
                // Specular and crushed pixels carry no usable channel ratio.
                if pixel.iter().any(|value| *value < 6 || *value > 249) {
                    continue;
                }
                for channel in 0..3 {
                    sum[channel] += pixel[channel] as f64;
                }
                count += 1;
            }
        }
        if count == 0 {
            return Err(ApiError::invalid_argument(
                "white balance sample is clipped",
                "这里太暗或过曝了，请换一块中性灰（例如灰卡、白墙）再试。",
            ));
        }

        let average = sum.map(|value| (value / count as f64).round().clamp(0.0, 255.0) as u8);
        let srgb = average.map(|value| value as f32 / 255.0);
        let (temperature, tint) = color_pipeline::neutral_white_balance(srgb).ok_or_else(|| {
            ApiError::invalid_argument(
                "white balance sample is unusable",
                "这个点不适合作为白平衡参考，请选择中性灰或白色区域。",
            )
        })?;
        Ok(WhiteBalanceResult {
            temperature,
            tint,
            sample: average,
        })
    }

    pub fn add_layer(&self, request: AddLayerRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.session(project_id, |project| {
            let face_context = request.kind == AddLayerKind::Portrait
                && (request.name.is_some() || request.portrait.is_some() || request.mask.is_some());
            let (mut layer, mut label) = match request.kind {
                AddLayerKind::Portrait => (Layer::portrait(), "添加人像图层"),
                AddLayerKind::Color => (Layer::color(), "添加调色图层"),
                AddLayerKind::Retouch => (Layer::retouch(), "添加修复图层"),
            };
            if face_context {
                label = "添加人脸美颜";
            }
            if let Some(name) = request.name {
                layer.name = name;
            }
            if let Some(portrait) = request.portrait {
                if request.kind != AddLayerKind::Portrait {
                    return Err(ApiError::invalid_argument(
                        "portrait params sent to a non-portrait layer",
                        "只有人像图层可以设置人像参数。",
                    ));
                }
                layer.portrait = Some(portrait);
            }
            if let Some(mask) = request.mask {
                if !matches!(request.kind, AddLayerKind::Portrait | AddLayerKind::Color) {
                    return Err(ApiError::invalid_argument(
                        "mask sent to a layer that cannot be masked",
                        "只有人像或调色图层支持蒙版。",
                    ));
                }
                layer.mask = Some(mask);
            }
            layer.sanitize();
            project
                .session
                .try_mutate(|document| document.add_layer(layer).map_err(ApiError::from))?;
            project.session.commit(label);
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn duplicate_layer(
        &self,
        request: DuplicateLayerRequest,
    ) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        self.session(project_id, |project| {
            project.session.try_mutate(|document| {
                document.duplicate_layer(layer_id).map_err(ApiError::from)
            })?;
            project.session.commit("复制图层");
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn update_layer(&self, request: UpdateLayerRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        let patch = &request.patch;
        // Opacity is dragged continuously. Like `update_layer_params`, it must
        // not create a history entry per debounce tick; the UI commits once on
        // release. Every other patch is a discrete action.
        let only_opacity = patch.opacity.is_some()
            && patch.name.is_none()
            && patch.visible.is_none()
            && patch.locked.is_none()
            && patch.blend_mode.is_none();
        let label = if let Some(visible) = patch.visible {
            if visible {
                "显示图层"
            } else {
                "隐藏图层"
            }
        } else if patch.blend_mode.is_some() {
            "更改混合方式"
        } else if patch.name.is_some() {
            "重命名图层"
        } else if let Some(locked) = patch.locked {
            if locked {
                "锁定图层"
            } else {
                "解锁图层"
            }
        } else {
            "编辑图层"
        };

        self.session(project_id, |project| {
            project.session.try_mutate(|document| {
                let layer = document
                    .layer_mut(layer_id)
                    .ok_or(DocumentError::LayerNotFound)?;
                layer.apply_patch(patch);
                Ok::<_, DocumentError>(())
            })?;
            if only_opacity {
                // Live opacity preview only; `commit_changes` persists/history.
            } else {
                project.session.commit(label);
                project.request_save();
            }
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn delete_layer(&self, request: DeleteLayerRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        self.session(project_id, |project| {
            project
                .session
                .try_mutate(|document| document.delete_layer(layer_id).map_err(ApiError::from))?;
            project.session.commit("删除图层");
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn move_layer(&self, request: MoveLayerRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        let direction: MoveDirection = request.direction;
        self.session(project_id, |project| {
            project.session.try_mutate(|document| {
                document
                    .move_layer(layer_id, direction)
                    .map_err(ApiError::from)
            })?;
            project.session.commit("调整图层顺序");
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn undo(&self, request: ProjectIdRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.session(project_id, |project| {
            if project.session.undo() {
                project.request_save();
            }
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn redo(&self, request: ProjectIdRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.session(project_id, |project| {
            if project.session.redo() {
                project.request_save();
            }
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn cancel_export(&self, request: CancelExportRequest) -> bool {
        let job_id = request.job_id.trim();
        if job_id.is_empty() {
            return false;
        }
        self.tasks.cancel(job_id)
    }

    pub fn export_image(&self, request: ExportImageRequest) -> Result<ExportResult, ApiError> {
        let job_id = request.job_id.clone();
        let task = job_id
            .as_deref()
            .map(|id| self.tasks.register(id, TaskPriority::Export));
        let token = task.as_ref().map(|task| task.token.clone());
        let result = self.export_image_tracked(request, token);
        if let Some(task) = task {
            self.tasks.finish(&task.id);
        }
        result
    }

    fn notify_export(&self, job_id: Option<&str>, phase: &str, percent: Option<u8>, message: &str) {
        if let Some(job_id) = job_id {
            self.events.export_progress(&ExportProgress {
                job_id: job_id.to_string(),
                phase: phase.to_string(),
                percent,
                message: message.to_string(),
            });
        }
    }

    fn export_stage(
        &self,
        job_id: Option<&str>,
        token: Option<&CancellationToken>,
        phase: &str,
        percent: u8,
        message: &str,
    ) -> Result<(), ApiError> {
        if token.is_some_and(CancellationToken::is_cancelled) {
            self.notify_export(job_id, "cancelled", None, "导出已取消。");
            return Err(ApiError::new(
                "EXPORT_CANCELLED",
                "export cancelled",
                "导出已取消。",
            ));
        }
        self.notify_export(job_id, phase, Some(percent), message);
        Ok(())
    }

    fn export_image_tracked(
        &self,
        request: ExportImageRequest,
        token: Option<CancellationToken>,
    ) -> Result<ExportResult, ApiError> {
        let job_id = request.job_id.clone();
        let started = Instant::now();
        let project_id = parse_uuid(&request.project_id, "project")?;
        let options = request.options;
        let target = PathBuf::from(options.path.trim());
        if target.as_os_str().is_empty() {
            return Err(ApiError::invalid_argument(
                "export path is empty",
                "请选择导出位置。",
            ));
        }
        if target.is_dir() {
            return Err(ApiError::invalid_argument(
                "export path is a directory",
                "导出路径必须是一个文件。",
            ));
        }
        let output_format = match options.format {
            ExportImageFormat::Jpeg => OutputFormat::Jpeg,
            ExportImageFormat::Png => OutputFormat::Png,
            ExportImageFormat::Tiff => OutputFormat::Tiff,
            ExportImageFormat::Webp => OutputFormat::Webp,
        };
        if !extension_matches(&target, options.format) {
            return Err(ApiError::invalid_argument(
                "export extension does not match format",
                "导出文件扩展名与所选格式不匹配。",
            ));
        }
        if options.bit_depth == ExportBitDepth::Sixteen
            && matches!(
                options.format,
                ExportImageFormat::Jpeg | ExportImageFormat::Webp
            )
        {
            return Err(ApiError::new(
                "BIT_DEPTH_UNSUPPORTED",
                "this format cannot store 16-bit samples",
                "JPEG 与 WebP 不支持 16 位导出，请改用 PNG 或 TIFF。",
            ));
        }
        if options.format == ExportImageFormat::Webp
            && options.color_space != ExportColorSpace::Srgb
        {
            return Err(ApiError::new(
                "COLOR_SPACE_UNSUPPORTED",
                "webp export only supports sRGB",
                "WebP 导出仅支持 sRGB，请切换色彩空间后再试。",
            ));
        }
        if options.max_dimension == Some(0) {
            return Err(ApiError::invalid_argument(
                "export max dimension is zero",
                "导出尺寸无效。",
            ));
        }
        let output_size = match (options.width, options.height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Some((width, height)),
            (None, None) => None,
            _ => {
                return Err(ApiError::invalid_argument(
                    "export width and height must be supplied together and be positive",
                    "自定义导出尺寸无效。",
                ))
            }
        };
        if let Some((width, height)) = output_size {
            if width as u64 * height as u64 > 100_000_000 {
                return Err(ApiError::invalid_argument(
                    "export dimensions exceed 100 megapixels",
                    "导出尺寸过大，请缩小到 1 亿像素以内。",
                ));
            }
        }
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(ApiError::invalid_argument(
                "export directory does not exist",
                "导出目录不存在，请选择其他位置。",
            ));
        }
        self.export_stage(
            job_id.as_deref(),
            token.as_ref(),
            "preparing",
            5,
            "准备导出…",
        )?;

        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        if let Some(source) = &document.source {
            if same_path(&target, Path::new(&source.original_path)) {
                return Err(ApiError::invalid_argument(
                    "export target would overwrite the original photo",
                    "不能覆盖原始照片，请选择其他文件名。",
                ));
            }
        }

        self.export_stage(
            job_id.as_deref(),
            token.as_ref(),
            "rendering",
            15,
            "读取源图…",
        )?;
        let source = self.load_source_image(project_id, &document)?;
        self.export_stage(
            job_id.as_deref(),
            token.as_ref(),
            "rendering",
            35,
            "正在渲染全分辨率画面…",
        )?;
        let resolver = DbAssetResolver {
            db: &self.db,
            luts: &self.luts,
        };
        let sixteen_bit = options.bit_depth == ExportBitDepth::Sixteen;
        let mut backend = "cpu";
        let mut raster = if sixteen_bit {
            // 16-bit output has to stay in the linear pipeline, which only the
            // CPU renderer produces; the GPU path returns 8-bit pixels.
            let rendered: LinearImage =
                render_document(&document, &source, options.max_dimension, &resolver)?;
            let rendered = match output_size {
                Some((width, height)) => rendered.resize_cover(width, height)?,
                None => rendered,
            };
            RasterImage::Sixteen(rendered.to_srgb16())
        } else if render_engine::gpu_available() {
            match render_engine::render_document_gpu(
                &document,
                &source,
                options.max_dimension,
                &resolver,
            ) {
                Some(Ok(image)) => {
                    backend = "wgpu";
                    RasterImage::Eight(image)
                }
                Some(Err(error)) => return Err(error.into()),
                None => {
                    let rendered: LinearImage =
                        render_document(&document, &source, options.max_dimension, &resolver)?;
                    RasterImage::Eight(rendered.to_srgb8())
                }
            }
        } else {
            let rendered: LinearImage =
                render_document(&document, &source, options.max_dimension, &resolver)?;
            RasterImage::Eight(rendered.to_srgb8())
        };
        self.export_stage(
            job_id.as_deref(),
            token.as_ref(),
            "encoding",
            70,
            "正在调整输出尺寸…",
        )?;
        if let Some((width, height)) = output_size {
            if let RasterImage::Eight(image) = &mut raster {
                *image = image.resize_cover(width, height)?;
            }
        }
        if let Some(watermark) = options.watermark.as_ref() {
            self.export_stage(
                job_id.as_deref(),
                token.as_ref(),
                "encoding",
                80,
                "正在添加水印…",
            )?;
            apply_export_watermark(&mut raster, watermark)?;
        }
        self.export_stage(
            job_id.as_deref(),
            token.as_ref(),
            "encoding",
            85,
            "正在编码文件…",
        )?;
        let (icc, written_space) =
            self.apply_export_color_space(&document, &mut raster, options.color_space)?;
        let (exif, mut metadata_written, keeps_location) =
            if options.format == ExportImageFormat::Webp {
                // The VP8L encoder carries neither EXIF nor an ICC profile.
                (None, false, false)
            } else {
                self.prepare_export_exif(&document, options.metadata)?
            };
        let existed = target.exists();
        let bytes = encode_atomic_image(
            &target,
            &raster,
            output_format,
            options.quality.clamp(1, 100),
            &EncodeOptions {
                icc: icc.as_deref(),
                exif: exif.as_deref(),
            },
        )?;
        // The TIFF encoder has no EXIF hook, so the block is injected after
        // encoding. Verify it landed instead of trusting the attempt.
        if output_format == OutputFormat::Tiff && exif.is_some() {
            metadata_written = image_core::tiff_file_has_exif(&target);
        }
        if token.as_ref().is_some_and(CancellationToken::is_cancelled) {
            if !existed {
                let _ = std::fs::remove_file(&target);
            }
            self.notify_export(job_id.as_deref(), "cancelled", None, "导出已取消。");
            return Err(ApiError::new(
                "EXPORT_CANCELLED",
                "export cancelled",
                "导出已取消。",
            ));
        }
        self.notify_export(job_id.as_deref(), "completed", Some(100), "导出完成。");
        Ok(ExportResult {
            path: target.to_string_lossy().to_string(),
            bytes,
            width: raster.width(),
            height: raster.height(),
            duration_ms: started.elapsed().as_millis() as u64,
            backend: backend.to_string(),
            color_space: written_space.to_string(),
            bit_depth: options.bit_depth.bits(),
            metadata_written,
            keeps_location,
        })
    }

    /// Build the EXIF block for an export. Returns the bytes to embed plus what
    /// actually happened, because TIFF cannot carry EXIF at all and the UI must
    /// not claim otherwise.
    fn prepare_export_exif(
        &self,
        document: &ProjectDocument,
        policy: ExportMetadata,
    ) -> Result<(Option<Vec<u8>>, bool, bool), ApiError> {
        if policy == ExportMetadata::None {
            return Ok((None, false, false));
        }
        let Some(source) = document.source.as_ref() else {
            return Ok((None, false, false));
        };
        if source.original_path.starts_with("browser://") {
            return Ok((None, false, false));
        }
        let path = Path::new(&source.original_path);
        let raw = match image_core::read_source_exif(path) {
            Ok(Some(blob)) => blob,
            // Missing metadata is normal for screenshots and PNGs.
            Ok(None) => return Ok((None, false, false)),
            Err(error) => {
                // A file we cannot read any more is reported by the render that
                // already succeeded, so dropping metadata here is not silent.
                return Err(error.into());
            }
        };
        let keeps_location = policy == ExportMetadata::Keep && image_core::has_gps(&raw);
        let prepared = image_core::prepare_exif(&raw, policy.into());
        Ok((prepared, true, keeps_location))
    }

    /// Map rendered working-space pixels into the requested output space and
    /// return the profile bytes to embed. Returns the space actually written,
    /// which differs from the request when there is no source profile to keep.
    fn apply_export_color_space(
        &self,
        document: &ProjectDocument,
        encoded: &mut RasterImage,
        requested: ExportColorSpace,
    ) -> Result<(Option<Vec<u8>>, &'static str), ApiError> {
        if requested == ExportColorSpace::Srgb {
            // Untagged is the universal convention for sRGB output.
            return Ok((None, "sRGB"));
        }
        let (profile_bytes, label) = match requested {
            ExportColorSpace::DisplayP3 => {
                let bytes = image_core::build_rgb_profile(
                    "Display P3",
                    image_core::DISPLAY_P3_PRIMARIES,
                    image_core::ProfileTrc::Srgb,
                )
                .ok_or_else(|| ApiError::internal("failed to synthesize the Display P3 profile"))?;
                (bytes, "Display P3")
            }
            ExportColorSpace::Source => {
                let Some(asset_id) = document.source.as_ref().and_then(|s| s.icc_asset_id) else {
                    return Ok((None, "sRGB"));
                };
                let record = self
                    .db
                    .get_asset(asset_id)?
                    .ok_or_else(|| ApiError::internal("source profile asset is missing"))?;
                let bytes = std::fs::read(&record.path)
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                (bytes, "原图配置")
            }
            ExportColorSpace::Srgb => unreachable!("handled above"),
        };
        let parsed = image_core::IccProfile::parse(&profile_bytes).ok_or_else(|| {
            ApiError::new(
                "ICC_INVALID",
                "colour profile could not be parsed",
                "色彩配置无法解析，已停止导出以免生成偏色文件。",
            )
        })?;
        if !parsed.can_convert() {
            return Err(ApiError::new(
                "ICC_UNSUPPORTED",
                "colour profile is not an RGB matrix/TRC profile",
                "这个色彩配置不是 RGB 配置，无法据此导出。",
            ));
        }
        match encoded {
            RasterImage::Eight(image) => parsed.convert_pixels_from_working(&mut image.rgba8),
            RasterImage::Sixteen(image) => {
                parsed.convert_pixels_from_working_16(&mut image.rgba);
            }
        }
        Ok((Some(profile_bytes), label))
    }

    pub fn analyze_portrait(
        &self,
        request: ProjectIdRequest,
    ) -> Result<PortraitAnalysisResult, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        let source = self.load_analysis_image(project_id, &document)?;
        let analyzer = portrait_engine::EnsembleFaceAnalyzer::new().map_err(ApiError::internal)?;
        let mut analysis = analyzer.analyze(&source);
        if analysis.available {
            let detection = super::CachedDetection {
                faces: analysis.faces.clone(),
                width: source.width,
                height: source.height,
            };
            self.session(project_id, |project| {
                project.detection = Some(detection);
                Ok(())
            })?;
        }
        if let Some(asset) = document.source.as_ref() {
            let scale_x = asset.width as f32 / source.width.max(1) as f32;
            let scale_y = asset.height as f32 / source.height.max(1) as f32;
            for face in &mut analysis.faces {
                face.x *= scale_x;
                face.y *= scale_y;
                face.width *= scale_x;
                face.height *= scale_y;
            }
        }
        Ok(PortraitAnalysisResult {
            available: analysis.available,
            faces: analysis.faces,
            warnings: analysis.warnings,
        })
    }

    /// Measure facial regions for the current source image. The result is
    /// cached per open project: detection takes hundreds of milliseconds and
    /// the decoded source never changes while the project stays open.
    pub fn analyze_portrait_regions(
        &self,
        request: ProjectIdRequest,
    ) -> Result<PortraitRegionsResult, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let (document, cached, cached_detection) = self.session(project_id, |project| {
            Ok((
                project.session.document().clone(),
                project.regions_analysed.then(|| project.regions.clone()),
                project
                    .detection
                    .as_ref()
                    .map(|detection| (detection.faces.clone(), detection.width, detection.height)),
            ))
        })?;
        let faces = match cached {
            Some(cached) => cached,
            None => {
                let analysis = self.load_analysis_image(project_id, &document)?;
                let faces = match cached_detection {
                    Some((faces, width, height))
                        if width == analysis.width && height == analysis.height =>
                    {
                        faces
                    }
                    _ => {
                        let analyzer = portrait_engine::EnsembleFaceAnalyzer::new()
                            .map_err(ApiError::internal)?;
                        let detection = analyzer.analyze(&analysis);
                        if detection.available {
                            let cached = super::CachedDetection {
                                faces: detection.faces.clone(),
                                width: analysis.width,
                                height: analysis.height,
                            };
                            self.session(project_id, |project| {
                                project.detection = Some(cached);
                                Ok(())
                            })?;
                        }
                        detection.faces
                    }
                };
                let measured = portrait_engine::regions::analyze_regions(&analysis, &faces);
                let source = document.source.as_ref().ok_or_else(|| {
                    ApiError::new(
                        "NO_SOURCE",
                        "project has no source image",
                        "这个项目还没有照片。",
                    )
                })?;
                let faces = portrait_regions_to_dto(
                    measured,
                    analysis.width,
                    analysis.height,
                    source.width,
                    source.height,
                );
                self.session(project_id, |project| {
                    project.regions = faces.clone();
                    project.regions_analysed = true;
                    Ok(())
                })?;
                faces
            }
        };
        let warnings = if faces.is_empty() {
            vec![PortraitWarning {
                code: "no_face_found".to_string(),
                message: "没有在这张照片中定位到五官，区域识别需要一张正对镜头的清晰人脸。"
                    .to_string(),
            }]
        } else {
            Vec::new()
        };
        Ok(PortraitRegionsResult {
            available: true,
            faces,
            warnings,
        })
    }
}

/// Convert measured pixel regions to the normalized DTO. Measurements happen
/// on the analysis proxy, so normalization must use that proxy's dimensions —
/// otherwise every region on a downscaled photo is too small and misplaced.
fn portrait_regions_to_dto(
    faces: Vec<portrait_engine::regions::FaceRegions>,
    analysis_width: u32,
    analysis_height: u32,
    source_width: u32,
    source_height: u32,
) -> Vec<PortraitFaceRegions> {
    let width = analysis_width.max(1) as f32;
    let height = analysis_height.max(1) as f32;
    let scale_x = source_width as f32 / width;
    let scale_y = source_height as f32 / height;
    faces
        .into_iter()
        .map(|face| {
            let mut boxed = face.face_box.clone();
            boxed.x *= scale_x;
            boxed.y *= scale_y;
            boxed.width *= scale_x;
            boxed.height *= scale_y;
            PortraitFaceRegions {
                face_id: face.face_id.clone(),
                face_box: boxed,
                keypoints: face
                    .keypoints
                    .iter()
                    .map(|keypoint| PortraitKeypoint {
                        kind: camel_kind(keypoint.kind),
                        label: keypoint.kind.label().to_string(),
                        x: keypoint.point.x / width,
                        y: keypoint.point.y / height,
                    })
                    .collect(),
                regions: face
                    .regions
                    .iter()
                    .map(|region| PortraitRegion {
                        kind: camel_kind(region.kind),
                        label: region.kind.label().to_string(),
                        center_x: region.shape.center.x / width,
                        center_y: region.shape.center.y / height,
                        radius_x: region.shape.radius_x / width,
                        radius_y: region.shape.radius_y / height,
                        angle: region.shape.angle,
                        feather: region.shape.feather,
                        confidence: region.shape.confidence,
                    })
                    .collect(),
                score: face.score,
            }
        })
        .collect()
}

fn extension_matches(path: &Path, format: ExportImageFormat) -> bool {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match format {
        ExportImageFormat::Jpeg => matches!(ext.as_deref(), Some("jpg") | Some("jpeg")),
        ExportImageFormat::Png => ext.as_deref() == Some("png"),
        ExportImageFormat::Tiff => matches!(ext.as_deref(), Some("tif") | Some("tiff")),
        ExportImageFormat::Webp => ext.as_deref() == Some("webp"),
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let normalized = match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(parent) => parent.join(name),
            Err(_) => absolute,
        },
        _ => absolute,
    };
    let text = normalized.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        text.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::{portrait_regions_to_dto, scale_crop_to_proxy};
    use ::document::ProjectDocument;
    use image_core::DecodedImage;
    use portrait_engine::analysis::FaceBox;
    use portrait_engine::regions::{
        DetectedRegion, FaceRegions, RegionKind, RegionPoint, RegionShape,
    };

    #[test]
    fn regions_are_normalized_with_the_analysis_proxy_not_the_source() {
        let faces = vec![FaceRegions {
            face_id: "face-1".into(),
            face_box: FaceBox {
                face_id: "face-1".into(),
                x: 100.0,
                y: 100.0,
                width: 80.0,
                height: 80.0,
                confidence: 12.0,
                models: Vec::new(),
            },
            regions: vec![DetectedRegion {
                kind: RegionKind::Skin,
                shape: RegionShape {
                    center: RegionPoint { x: 800.0, y: 400.0 },
                    radius_x: 80.0,
                    radius_y: 40.0,
                    angle: 0.0,
                    feather: 0.4,
                    confidence: 0.8,
                },
            }],
            keypoints: Vec::new(),
            score: 0.8,
        }];
        let dto = portrait_regions_to_dto(faces, 1600, 800, 4000, 2000);
        let face = &dto[0];
        assert!((face.regions[0].center_x - 0.5).abs() < 1e-6);
        assert!((face.regions[0].center_y - 0.5).abs() < 1e-6);
        assert!((face.regions[0].radius_x - 0.05).abs() < 1e-6);
        assert!((face.regions[0].radius_y - 0.05).abs() < 1e-6);
        assert!((face.face_box.x - 250.0).abs() < 1e-6);
        assert!((face.face_box.width - 200.0).abs() < 1e-6);
    }

    fn document_with_crop(x: u32, y: u32, width: u32, height: u32) -> ProjectDocument {
        let mut document = ProjectDocument::new("proxy", 4000, 2000);
        document.crop.x = x;
        document.crop.y = y;
        document.crop.width = width;
        document.crop.height = height;
        document.crop.angle = 0.0;
        document
    }

    fn proxy_of(width: u32, height: u32) -> DecodedImage {
        DecodedImage::new(width, height, vec![0u8; (width * height * 4) as usize]).unwrap()
    }

    #[test]
    fn crop_is_mapped_onto_the_preview_proxy() {
        let document = document_with_crop(1000, 500, 2000, 1000);
        let scaled = scale_crop_to_proxy(&document, &proxy_of(1600, 800), 4000, 2000);
        // 4000x2000 -> 1600x800 is 0.4, so every crop edge follows.
        assert_eq!(
            (
                scaled.crop.x,
                scaled.crop.y,
                scaled.crop.width,
                scaled.crop.height
            ),
            (400, 200, 800, 400)
        );
    }

    #[test]
    fn crop_is_untouched_when_the_proxy_matches_the_source() {
        let document = document_with_crop(1000, 500, 2000, 1000);
        let scaled = scale_crop_to_proxy(&document, &proxy_of(4000, 2000), 4000, 2000);
        assert_eq!(
            (
                scaled.crop.x,
                scaled.crop.y,
                scaled.crop.width,
                scaled.crop.height
            ),
            (1000, 500, 2000, 1000)
        );
    }

    #[test]
    fn crop_is_clamped_inside_a_smaller_proxy() {
        let document = document_with_crop(3900, 1900, 400, 400);
        let scaled = scale_crop_to_proxy(&document, &proxy_of(1600, 800), 4000, 2000);
        assert!(scaled.crop.x + scaled.crop.width <= 1600);
        assert!(scaled.crop.y + scaled.crop.height <= 800);
        assert!(scaled.crop.width >= 1 && scaled.crop.height >= 1);
    }
}
