//! DTOs at the IPC boundary. Field names mirror `docs/ipc-contract.md` exactly
//! (serde camelCase).

use document::{ColorParams, LayerMask, PortraitParams};
use portrait_engine::{FaceBox, PortraitWarning};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub backend: String,
    pub gpu_available: bool,
    pub raw_available: bool,
    pub face_model_available: bool,
    pub workspace_dir: String,
    pub supported_image_formats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEngineCapability {
    pub available: bool,
    /// User-facing reason when unavailable.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDenoiseCapability {
    pub photo_hq: AiEngineCapability,
    pub onnx: AiEngineCapability,
}

/// Whether the configured segmentation model can actually run. `reason` is
/// shown verbatim in the UI, so an unconfigured model never looks like a bug.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentationCapability {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub collection_id: String,
    pub collection_name: String,
    pub width: u32,
    pub height: u32,
    pub updated_at: String,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub photo_count: usize,
    pub updated_at: String,
    pub cover_thumbnail_path: Option<String>,
    pub latest_project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncColorAdjustmentsResult {
    /// Projects that now carry the source colour stack.
    pub synced: usize,
    /// Display names of projects that were skipped or could not be updated.
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAssetState {
    pub asset_id: String,
    pub display_name: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub original_path: String,
    pub preview_path: String,
    pub thumbnail_path: String,
    /// Colour space the file declared, e.g. `sRGB`, `Display P3`.
    pub color_space: String,
    /// True when import remapped the pixels and kept the original profile,
    /// which is what makes "export in the source space" possible.
    pub has_source_profile: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasState {
    pub width: u32,
    pub height: u32,
    pub working_color_space: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerState {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend_mode: String,
    pub color: Option<ColorParams>,
    pub portrait: Option<PortraitParams>,
    pub retouch: Option<document::RetouchParams>,
    pub mask: Option<LayerMask>,
    pub asset_id: Option<String>,
    pub preview_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropState {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Clockwise straighten angle in degrees; the crop lives in the rotated frame.
    pub angle: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentState {
    pub id: String,
    pub name: String,
    pub collection_id: String,
    pub collection_name: String,
    pub schema_version: u32,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub canvas: CanvasState,
    pub crop: CropState,
    pub source: Option<SourceAssetState>,
    pub layers: Vec<LayerState>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub history: Vec<String>,
}

/// A named snapshot of a project document.
///
/// The document payload stays in SQLite; the UI only needs identity, a label, a
/// thumbnail and the revision it was captured at.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub created_at: String,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheClearResult {
    pub deleted_files: usize,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub duration_ms: u64,
    pub cached: bool,
    /// Actual renderer used: `wgpu`, `cpu`, or `cache`.
    pub backend: String,
    /// RGB/luma distribution of the rendered preview, 64 bins per channel.
    pub histogram: Option<Histogram>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Histogram {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub luma: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoToneResult {
    pub exposure: f32,
    pub contrast: f32,
    pub whites: f32,
    pub blacks: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhiteBalanceResult {
    /// Suggested `ColorParams::temperature`, -100..100.
    pub temperature: f32,
    /// Suggested `ColorParams::tint`, -100..100.
    pub tint: f32,
    /// Average sRGB of the sampled neighborhood, for the UI swatch.
    pub sample: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LutAssetResult {
    pub asset_id: String,
    pub display_name: String,
    /// `1D` or `3D`, from the cube header.
    pub dimension: String,
    /// Entries along one axis: cube edge length for 3D, table length for 1D.
    pub size: usize,
    pub file_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportImageFormat {
    Jpeg,
    Png,
    Tiff,
    Webp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExportColorSpace {
    #[default]
    Srgb,
    #[serde(rename = "displayP3")]
    DisplayP3,
    /// Write the photo back in the profile it was imported from.
    Source,
}

impl ExportColorSpace {
    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::Source => "原图配置",
        }
    }
}

/// How much of the source EXIF an export should carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExportMetadata {
    /// Camera and shooting tags, with the baked orientation and the stale
    /// embedded thumbnail removed.
    #[default]
    Keep,
    /// `Keep` plus removal of location data.
    Privacy,
    /// No EXIF at all.
    None,
}

impl From<ExportMetadata> for image_core::MetadataPolicy {
    fn from(value: ExportMetadata) -> Self {
        match value {
            ExportMetadata::Keep => Self::Keep,
            ExportMetadata::Privacy => Self::Privacy,
            ExportMetadata::None => Self::None,
        }
    }
}

impl ExportMetadata {
    pub fn label(self) -> &'static str {
        match self {
            Self::Keep => "保留拍摄信息",
            Self::Privacy => "保留但去除位置",
            Self::None => "不写入",
        }
    }
}

/// Sample depth of the exported raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExportBitDepth {
    #[default]
    Eight,
    /// PNG and TIFF only; keeps the linear render's precision.
    Sixteen,
}

impl ExportBitDepth {
    pub fn bits(self) -> u8 {
        match self {
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub path: String,
    pub format: ExportImageFormat,
    pub quality: u32,
    #[serde(default)]
    pub max_dimension: Option<u32>,
    /// Exact output size after a cover-crop. Both values are required together.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Colour space written into the file. Defaults to sRGB.
    #[serde(default)]
    pub color_space: ExportColorSpace,
    /// EXIF policy for the exported file.
    #[serde(default)]
    pub metadata: ExportMetadata,
    /// 8- or 16-bit output. 16-bit requires PNG or TIFF.
    #[serde(default)]
    pub bit_depth: ExportBitDepth,
    /// Optional logo/text watermark, already rasterized by the UI.
    #[serde(default)]
    pub watermark: Option<WatermarkOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WatermarkPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
    Center,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkOptions {
    /// PNG/JPEG data URL or bare base64. Text is rasterized by the UI so the
    /// backend needs no font stack.
    pub image_base64: String,
    /// 0..1 blend applied on top of the image's own alpha.
    #[serde(default = "default_watermark_opacity")]
    pub opacity: f32,
    /// Watermark width as a fraction of the exported shortest edge.
    #[serde(default = "default_watermark_scale")]
    pub scale: f32,
    /// Edge distance as a fraction of the exported shortest edge.
    #[serde(default = "default_watermark_margin")]
    pub margin: f32,
    #[serde(default)]
    pub position: WatermarkPosition,
}

fn default_watermark_opacity() -> f32 {
    0.6
}

fn default_watermark_scale() -> f32 {
    0.18
}

fn default_watermark_margin() -> f32 {
    0.04
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub backend: String,
    /// Colour space actually written, which can differ from the request when
    /// the photo has no source profile to preserve.
    pub color_space: String,
    /// Sample depth actually written.
    pub bit_depth: u8,
    /// True when EXIF really made it into the file (TIFF cannot carry any).
    pub metadata_written: bool,
    /// True when the retained EXIF still contains location data.
    pub keeps_location: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub job_id: String,
    /// `preparing` | `rendering` | `encoding` | `completed` | `cancelled`.
    pub phase: String,
    /// 0..100. Null means the phase cannot report a precise value.
    pub percent: Option<u8>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitAnalysisResult {
    pub available: bool,
    pub faces: Vec<FaceBox>,
    pub warnings: Vec<PortraitWarning>,
}

/// One measured facial region, normalized to the uncropped source image so the
/// overlay and the layer mask share one coordinate space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitRegion {
    pub kind: String,
    pub label: String,
    pub center_x: f32,
    pub center_y: f32,
    pub radius_x: f32,
    pub radius_y: f32,
    pub angle: f32,
    pub feather: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitKeypoint {
    pub kind: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitFaceRegions {
    pub face_id: String,
    #[serde(rename = "box")]
    pub face_box: FaceBox,
    pub keypoints: Vec<PortraitKeypoint>,
    pub regions: Vec<PortraitRegion>,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitRegionsResult {
    pub available: bool,
    pub faces: Vec<PortraitFaceRegions>,
    pub warnings: Vec<PortraitWarning>,
}
