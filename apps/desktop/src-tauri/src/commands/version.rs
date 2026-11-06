use app_core::{
    ApiError, CreateVersionRequest, DocumentState, PreviewResult, ProjectIdRequest,
    RenderVersionPreviewRequest, VersionIdRequest, VersionSummary,
};
use tauri::State;

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn list_versions(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<Vec<VersionSummary>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.list_versions(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn create_version(
    state: State<'_, AppState>,
    req: CreateVersionRequest,
) -> Result<Vec<VersionSummary>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.create_version(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn restore_version(
    state: State<'_, AppState>,
    req: VersionIdRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.restore_version(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn render_version_preview(
    state: State<'_, AppState>,
    req: RenderVersionPreviewRequest,
) -> Result<PreviewResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.render_version_preview(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn delete_version(
    state: State<'_, AppState>,
    req: VersionIdRequest,
) -> Result<Vec<VersionSummary>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.delete_version(req))
        .await
        .map_err(join_error)?
}
