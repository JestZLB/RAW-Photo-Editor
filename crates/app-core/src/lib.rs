//! Application service layer for the Luma Portrait editor.
//!
//! The Tauri command layer is a thin adapter over [`AppService`]. Everything
//! that can touch disk, decode pixels or call a provider lives here so it can be
//! exercised by integration tests without a window.

mod dto;
mod error;
mod requests;
mod service;

pub use ai_provider::{
    AiJobInfo, AiJobOperation, AiJobStatus, ProviderCapabilities, ProviderConfigInput,
    ProviderInfo, ProviderKind, ProviderTestResult,
};
pub use dto::{
    AiDenoiseCapability, AiEngineCapability, AppInfo, AutoToneResult, CacheClearResult,
    CanvasState, CollectionSummary, CropState, DocumentState, ExportBitDepth, ExportColorSpace,
    ExportImageFormat, ExportMetadata, ExportOptions, ExportProgress, ExportResult, Histogram,
    LayerState, LutAssetResult, PortraitAnalysisResult, PortraitFaceRegions, PortraitKeypoint,
    PortraitRegion, PortraitRegionsResult, PreviewResult, ProjectSummary, SourceAssetState,
    SyncColorAdjustmentsResult, WatermarkOptions, WatermarkPosition, WhiteBalanceResult,
};
pub use error::ApiError;
pub use requests::{
    AddLayerKind, AddLayerRequest, AiCapabilityRequest, AiDenoiseRequest, ApplyAiResultRequest,
    CancelAiJobRequest, CancelExportRequest, CommitChangesRequest, CreateProjectRequest,
    DeleteLayerRequest, DuplicateLayerRequest, ExportImageRequest, ImportLutRequest,
    ImportPhotoBytesRequest, ImportPhotoRequest, ListAiJobsRequest, MoveLayerRequest,
    OpenProjectRequest, PhotoHqDenoiseRequest, PickWhiteBalanceRequest, ProjectIdRequest,
    ProviderIdRequest, RenameCollectionRequest, RenameProjectRequest, RenderPreviewRequest,
    SaveProviderConfigRequest, SetCropRequest, SetLayerMaskRequest, SetProviderCredentialRequest,
    SubmitAiJobRequest, SubmitAiJobResponse, SyncColorAdjustmentsRequest, TestProviderRequest,
    UpdateLayerParamsRequest, UpdateLayerRequest,
};
pub use service::{
    AppPaths, AppService, EventSink, NoopEventSink, DEFAULT_PREVIEW_MAX, MAX_IMPORT_BYTES,
    MAX_OPEN_PROJECTS, MAX_PREVIEW_DIMENSION, SOURCE_PREVIEW_MAX, SOURCE_THUMBNAIL_MAX,
};
