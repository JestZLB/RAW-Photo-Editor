use app_core::{
    ApiError, ProviderConfigInput, ProviderIdRequest, ProviderInfo, ProviderTestResult,
    SetProviderCredentialRequest, TestProviderRequest,
};
use tauri::State;

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderInfo>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.list_providers())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn save_provider_config(
    state: State<'_, AppState>,
    req: ProviderConfigInput,
) -> Result<ProviderInfo, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.save_provider_config(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn set_provider_credential(
    state: State<'_, AppState>,
    req: SetProviderCredentialRequest,
) -> Result<ProviderInfo, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.set_provider_credential(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    req: TestProviderRequest,
) -> Result<ProviderTestResult, ApiError> {
    let service = state.service.clone();
    service.test_provider(req).await
}

#[tauri::command]
pub async fn list_provider_models(
    state: State<'_, AppState>,
    req: ProviderIdRequest,
) -> Result<Vec<String>, ApiError> {
    let service = state.service.clone();
    service.list_provider_models(req).await
}
