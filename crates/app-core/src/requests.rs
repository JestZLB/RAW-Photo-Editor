//! IPC request/response payloads. Every struct is camelCase so the Tauri
//! command layer can deserialize `{ req }` without translation.

use document::{
    ColorParams, CropRect, LayerMask, LayerPatch, MoveDirection, PortraitParams, RetouchParams,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIdRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenProjectRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVersionRequest {
    pub project_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionIdRequest {
    pub project_id: String,
    pub version_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderVersionPreviewRequest {
    pub project_id: String,
    pub version_id: String,
    #[serde(default)]
    pub max_dimension: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameProjectRequest {
    pub project_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameCollectionRequest {
    pub collection_id: String,
    pub name: String,
}

/// Copy every colour layer of the source photo onto other photos in the same
/// set. Crop, portrait and retouch layers stay untouched: they are tied to the
/// source photo's composition and measured faces.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncColorAdjustmentsRequest {
    pub source_project_id: String,
    pub project_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPhotoRequest {
    pub path: String,
    /// Add the photo to an existing project/photo set instead of creating one.
    #[serde(default)]
    pub collection_id: Option<String>,
    /// Name for a newly created project/photo set.
    #[serde(default)]
    pub collection_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPhotoBytesRequest {
    pub file_name: String,
    pub data_base64: String,
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub collection_name: Option<String>,
}

/// Imports a `.cube` LUT into a project. Exactly one of `path` (native file
/// dialog) or `dataBase64` (web demo) must be provided.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLutRequest {
    pub project_id: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub data_base64: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickWhiteBalanceRequest {
    pub project_id: String,
    /// Normalized coordinates in the original (uncropped) source image.
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPreviewRequest {
    pub project_id: String,
    #[serde(default)]
    pub max_dimension: Option<u32>,
    /// Render the full uncropped (and unrotated) frame, used by the crop tool.
    #[serde(default)]
    pub ignore_crop: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLayerParamsRequest {
    pub project_id: String,
    pub layer_id: String,
    #[serde(default)]
    pub color: Option<ColorParams>,
    #[serde(default)]
    pub portrait: Option<PortraitParams>,
    #[serde(default)]
    pub retouch: Option<RetouchParams>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitChangesRequest {
    pub project_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateLayerRequest {
    pub project_id: String,
    pub layer_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLayerMaskRequest {
    pub project_id: String,
    pub layer_id: String,
    pub mask: Option<LayerMask>,
}

/// Run a local ONNX denoiser on the source image.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiDenoiseRequest {
    pub project_id: String,
    /// Absolute path to the `.onnx` model chosen in the settings page.
    pub model_path: String,
    /// 0..1 blend between the original and the model output.
    #[serde(default)]
    pub strength: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoHqDenoiseRequest {
    pub project_id: String,
    /// 0..1 blend between the original and the 4x model output.
    #[serde(default)]
    pub strength: Option<f32>,
    /// Optional client-generated id used to cancel a running engine.
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiCapabilityRequest {
    /// The ONNX file currently configured in settings, if any.
    #[serde(default)]
    pub onnx_model_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCropRequest {
    pub project_id: String,
    pub crop: CropRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddLayerKind {
    Portrait,
    Color,
    Retouch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLayerRequest {
    pub project_id: String,
    pub kind: AddLayerKind,
    /// Optional display name, used by per-face portrait layers.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional initial portrait parameters (portrait layers only).
    #[serde(default)]
    pub portrait: Option<PortraitParams>,
    /// Optional initial mask, used to constrain a per-face layer.
    #[serde(default)]
    pub mask: Option<LayerMask>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLayerRequest {
    pub project_id: String,
    pub layer_id: String,
    pub patch: LayerPatch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLayerRequest {
    pub project_id: String,
    pub layer_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveLayerRequest {
    pub project_id: String,
    pub layer_id: String,
    pub direction: MoveDirection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportImageRequest {
    pub project_id: String,
    pub options: crate::ExportOptions,
    /// Optional client-generated job id used for progress events and cancellation.
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelExportRequest {
    pub job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetProviderCredentialRequest {
    pub provider_id: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdRequest {
    pub provider_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderRequest {
    pub provider_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitAiJobRequest {
    pub project_id: String,
    pub provider_id: String,
    pub operation: ai_provider::AiJobOperation,
    pub prompt: String,
    pub strength: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelAiJobRequest {
    pub job_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAiJobsRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAiResultRequest {
    pub project_id: String,
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitAiJobResponse {
    pub job_id: String,
}

/// `save_provider_config` sends the config object itself as `req`.
pub type SaveProviderConfigRequest = ai_provider::ProviderConfigInput;
