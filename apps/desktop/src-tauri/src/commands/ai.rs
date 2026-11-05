use std::path::PathBuf;

use app_core::{
    AiCapabilityRequest, AiDenoiseCapability, AiDenoiseRequest, AiJobInfo, ApiError,
    ApplyAiResultRequest, CancelAiJobRequest, DocumentState, ListAiJobsRequest,
    PhotoHqDenoiseRequest, SubmitAiJobRequest, SubmitAiJobResponse,
};
use tauri::{Manager, State};

use super::join_error;
use crate::state::AppState;

#[tauri::command]
pub async fn submit_ai_job(
    state: State<'_, AppState>,
    req: SubmitAiJobRequest,
) -> Result<SubmitAiJobResponse, ApiError> {
    let service = state.service.clone();
    service.submit_ai_job(req).await
}

#[tauri::command]
pub async fn cancel_ai_job(
    state: State<'_, AppState>,
    req: CancelAiJobRequest,
) -> Result<(), ApiError> {
    state.service.cancel_ai_job(req)
}

#[tauri::command]
pub async fn list_ai_jobs(
    state: State<'_, AppState>,
    req: ListAiJobsRequest,
) -> Result<Vec<AiJobInfo>, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.list_ai_jobs(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn apply_ai_result(
    state: State<'_, AppState>,
    req: ApplyAiResultRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.apply_ai_result(req))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn run_ai_denoise(
    state: State<'_, AppState>,
    req: AiDenoiseRequest,
) -> Result<DocumentState, ApiError> {
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.run_ai_denoise(req))
        .await
        .map_err(join_error)?
}

/// Resolve the bundled Real-ESRGAN runtime, allowing a user override before
/// falling back to the resource directory and finally the dev checkout.
fn resolve_photo_hq_engine(app: &tauri::AppHandle) -> Option<PathBuf> {
    let has_engine = |dir: &PathBuf| dir.join("realesrgan-ncnn-vulkan.exe").is_file();
    if let Some(dir) = std::env::var_os("LUMA_REALESRGAN_DIR") {
        let dir = PathBuf::from(dir);
        if has_engine(&dir) {
            return Some(dir);
        }
    }
    if let Ok(local) = app.path().app_local_data_dir() {
        let dir = local.join("engines").join("realesrgan");
        if has_engine(&dir) {
            return Some(dir);
        }
    }
    if let Ok(resource) = app.path().resource_dir() {
        for relative in ["resources/realesrgan", "realesrgan"] {
            let dir = resource.join(relative);
            if has_engine(&dir) {
                return Some(dir);
            }
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("realesrgan");
    has_engine(&dev).then_some(dev)
}

#[tauri::command]
pub async fn run_photo_hq_denoise(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: PhotoHqDenoiseRequest,
) -> Result<DocumentState, ApiError> {
    let engine_dir = resolve_photo_hq_engine(&app).ok_or_else(|| {
        ApiError::new(
            "AI_DENOISE_ENGINE_MISSING",
            "photo-hq engine directory was not found",
            "没有找到 Photo-HQ 本地引擎，请重新安装应用。",
        )
    })?;
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || service.run_photo_hq_denoise(req, &engine_dir))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn cancel_ai_denoise(
    state: State<'_, AppState>,
    req: CancelAiJobRequest,
) -> Result<bool, ApiError> {
    Ok(state.service.cancel_ai_denoise(req))
}

#[tauri::command]
pub async fn check_ai_denoise_capability(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: AiCapabilityRequest,
) -> Result<AiDenoiseCapability, ApiError> {
    let engine_dir = resolve_photo_hq_engine(&app);
    let service = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok(service.ai_denoise_capability(req, engine_dir.as_deref()))
    })
    .await
    .map_err(join_error)?
}
