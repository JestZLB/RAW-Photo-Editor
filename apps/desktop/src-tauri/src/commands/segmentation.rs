use app_core::{
    ApiError, ClearRasterMaskRequest, DocumentState, SegmentMaskRequest, SegmentationCapability,
    SegmentationCapabilityRequest,
};
use tauri::State;

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn check_segmentation_capability(
    state: State<'_, AppState>,
    req: SegmentationCapabilityRequest,
) -> Result<SegmentationCapability, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.check_segmentation_capability(req))
        .await
        .map_err(join_error)
}

#[tauri::command]
pub async fn run_segmentation_mask(
    state: State<'_, AppState>,
    req: SegmentMaskRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.run_segmentation_mask(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn clear_raster_mask(
    state: State<'_, AppState>,
    req: ClearRasterMaskRequest,
) -> Result<DocumentState, ApiError> {
    // Separate from `set_layer_mask`, which deliberately keeps raster data when
    // the incoming vector mask omits it.
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.clear_raster_mask(req))
        .await
        .map_err(join_error)?
}
