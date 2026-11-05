use app_core::{
    AddLayerRequest, ApiError, AutoToneResult, CancelExportRequest, CommitChangesRequest,
    DeleteLayerRequest, DocumentState, DuplicateLayerRequest, ExportImageRequest, ExportResult,
    MoveLayerRequest, PickWhiteBalanceRequest, PortraitAnalysisResult, PortraitRegionsResult,
    ProjectIdRequest, RenderPreviewRequest, SetCropRequest, SetLayerMaskRequest,
    UpdateLayerParamsRequest, UpdateLayerRequest, WhiteBalanceResult,
};
use tauri::State;

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn render_preview(
    state: State<'_, AppState>,
    req: RenderPreviewRequest,
) -> Result<app_core::PreviewResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.render_preview(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn update_layer_params(
    state: State<'_, AppState>,
    req: UpdateLayerParamsRequest,
) -> Result<DocumentState, ApiError> {
    // In-memory only: no autosave/history until `commit_changes`. This still
    // runs on the blocking pool: it clones and serialises the whole document,
    // and sliders call it every ~70 ms while dragging.
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.update_layer_params(req))
        .await
        .map_err(join_error)?
}

/// Interactive drag path: returns the document envelope without the per-layer
/// payload, which the caller does not merge.
#[tauri::command]
pub async fn update_layer_params_meta(
    state: State<'_, AppState>,
    req: UpdateLayerParamsRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.update_layer_params_meta(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn set_layer_mask(
    state: State<'_, AppState>,
    req: SetLayerMaskRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.set_layer_mask(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn commit_changes(
    state: State<'_, AppState>,
    req: CommitChangesRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.commit_changes(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn set_crop(
    state: State<'_, AppState>,
    req: SetCropRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.set_crop(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn auto_tone(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<AutoToneResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.auto_tone(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn pick_white_balance(
    state: State<'_, AppState>,
    req: PickWhiteBalanceRequest,
) -> Result<WhiteBalanceResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.pick_white_balance(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn add_layer(
    state: State<'_, AppState>,
    req: AddLayerRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.add_layer(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn update_layer(
    state: State<'_, AppState>,
    req: UpdateLayerRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.update_layer(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn duplicate_layer(
    state: State<'_, AppState>,
    req: DuplicateLayerRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.duplicate_layer(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub fn cancel_export(state: State<'_, AppState>, req: CancelExportRequest) -> bool {
    state.service.cancel_export(req)
}

#[tauri::command]
pub async fn delete_layer(
    state: State<'_, AppState>,
    req: DeleteLayerRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.delete_layer(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn move_layer(
    state: State<'_, AppState>,
    req: MoveLayerRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.move_layer(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn undo(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.undo(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn redo(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.redo(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn export_image(
    state: State<'_, AppState>,
    req: ExportImageRequest,
) -> Result<ExportResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.export_image(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn analyze_portrait(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<PortraitAnalysisResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.analyze_portrait(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn analyze_portrait_regions(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<PortraitRegionsResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.analyze_portrait_regions(req))
        .await
        .map_err(join_error)?
}
