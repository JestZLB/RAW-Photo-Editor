mod ai;
mod app;
mod document;
mod project;
mod provider;
mod version;

pub use ai::*;
pub use app::*;
pub use document::*;
pub use project::*;
pub use provider::*;
pub use version::*;

use app_core::ApiError;

pub(crate) fn join_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("background task failed: {error}"))
}
