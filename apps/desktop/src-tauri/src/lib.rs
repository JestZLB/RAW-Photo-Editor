mod commands;
mod state;

use std::sync::Arc;

use app_core::{AiJobInfo, AppPaths, AppService, EventSink};
use tauri::{Emitter, Manager};

use crate::state::AppState;

/// Bridges app-core job progress to the webview `ai-job-updated` event.
struct TauriEventSink {
    app: tauri::AppHandle,
}

impl EventSink for TauriEventSink {
    fn ai_job_updated(&self, job: &AiJobInfo) {
        // Event delivery failures are non-fatal: the UI also polls
        // `list_ai_jobs`, and progress must never crash a background task.
        let _ = self.app.emit("ai-job-updated", job);
    }

    fn export_progress(&self, progress: &app_core::ExportProgress) {
        let _ = self.app.emit("export-progress", progress);
    }
}

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let events = Arc::new(TauriEventSink {
                app: app.handle().clone(),
            });
            let service = AppService::with_event_sink(AppPaths::new(data_dir, cache_dir), events)?;
            app.manage(AppState::new(service));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_projects,
            commands::list_collections,
            commands::create_project,
            commands::open_project,
            commands::rename_project,
            commands::rename_collection,
            commands::sync_color_adjustments,
            commands::delete_project,
            commands::clear_render_cache,
            commands::import_photo,
            commands::import_photo_bytes,
            commands::import_lut,
            commands::render_preview,
            commands::update_layer_params,
            commands::update_layer_params_meta,
            commands::commit_changes,
            commands::set_layer_mask,
            commands::pick_white_balance,
            commands::auto_tone,
            commands::set_crop,
            commands::add_layer,
            commands::update_layer,
            commands::duplicate_layer,
            commands::delete_layer,
            commands::move_layer,
            commands::undo,
            commands::redo,
            commands::export_image,
            commands::cancel_export,
            commands::analyze_portrait,
            commands::analyze_portrait_regions,
            commands::list_providers,
            commands::save_provider_config,
            commands::set_provider_credential,
            commands::test_provider,
            commands::list_provider_models,
            commands::submit_ai_job,
            commands::cancel_ai_job,
            commands::list_ai_jobs,
            commands::apply_ai_result,
            commands::run_ai_denoise,
            commands::run_photo_hq_denoise,
            commands::cancel_ai_denoise,
            commands::check_ai_denoise_capability,
            commands::list_versions,
            commands::create_version,
            commands::restore_version,
            commands::delete_version,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("Luma Portrait failed to start: {error}");
        std::process::exit(1);
    }
}
