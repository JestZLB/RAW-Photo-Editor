use app_core::{ApiError, AppInfo, CollectionSummary, ProjectSummary};
use tauri::State;

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> Result<AppInfo, ApiError> {
    Ok(state.service.get_app_info())
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.list_projects())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn list_collections(
    state: State<'_, AppState>,
) -> Result<Vec<CollectionSummary>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.list_collections())
        .await
        .map_err(join_error)?
}
