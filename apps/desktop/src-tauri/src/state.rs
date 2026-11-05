use std::sync::Arc;

use app_core::AppService;

/// Shared application state. Commands access the service through this handle;
/// no pixel buffers or secrets are exposed to the webview.
pub struct AppState {
    pub service: Arc<AppService>,
}

impl AppState {
    pub fn new(service: Arc<AppService>) -> Self {
        Self { service }
    }
}
