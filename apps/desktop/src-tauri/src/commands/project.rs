use app_core::{
    ApiError, CacheClearResult, CreateProjectRequest, DocumentState, ImportLutRequest,
    ImportPhotoBytesRequest, ImportPhotoRequest, LutAssetResult, OpenProjectRequest,
    ProjectIdRequest, ProjectSummary, RenameCollectionRequest, RenameProjectRequest,
    SyncColorAdjustmentsRequest, SyncColorAdjustmentsResult,
};
use tauri::State;

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    req: CreateProjectRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.create_project(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn open_project(
    state: State<'_, AppState>,
    req: OpenProjectRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.open_project(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn rename_project(
    state: State<'_, AppState>,
    req: RenameProjectRequest,
) -> Result<ProjectSummary, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.rename_project(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn rename_collection(
    state: State<'_, AppState>,
    req: RenameCollectionRequest,
) -> Result<(), ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.rename_collection(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn sync_color_adjustments(
    state: State<'_, AppState>,
    req: SyncColorAdjustmentsRequest,
) -> Result<SyncColorAdjustmentsResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.sync_color_adjustments(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn delete_project(
    state: State<'_, AppState>,
    req: ProjectIdRequest,
) -> Result<(), ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.delete_project(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn clear_render_cache(state: State<'_, AppState>) -> Result<CacheClearResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.clear_render_cache())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn import_photo(
    state: State<'_, AppState>,
    req: ImportPhotoRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.import_photo(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn import_photo_bytes(
    state: State<'_, AppState>,
    req: ImportPhotoBytesRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.import_photo_bytes(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn import_lut(
    state: State<'_, AppState>,
    req: ImportLutRequest,
) -> Result<LutAssetResult, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.import_lut(req))
        .await
        .map_err(join_error)?
}
