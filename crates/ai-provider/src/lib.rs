//! Provider abstraction. Only the local mock is exercised end-to-end; the
//! OpenAI-compatible provider is implemented against the documented image-edit
//! API and URL validation is unit-tested without network access.

mod error;
mod mock;
mod models;
mod openai;
mod registry;

pub use error::{ProviderError, ProviderErrorCode};
pub use mock::MockProvider;
pub use models::{
    now_rfc3339, AiJobInfo, AiJobOperation, AiJobRequest, AiJobStatus, ImageEditProvider,
    ProgressCallback, ProviderCapabilities, ProviderConfigInput, ProviderInfo, ProviderKind,
    ProviderOutput, ProviderTestResult, SecretString,
};
pub use openai::{
    validate_base_url, validate_result_url, OpenAiCompatibleProvider, OpenAiProviderConfig,
    MAX_API_RESPONSE_BYTES, MAX_RESULT_IMAGE_BYTES,
};
pub use registry::{
    ProviderRegistry, StoredProviderConfig, MOCK_PROVIDER_ID, MOCK_PROVIDER_NAME,
    OPENAI_MAX_INPUT_BYTES,
};

use image_core::ImageError;

pub(crate) fn map_input_image_error(error: ImageError) -> ProviderError {
    let code = match error.code() {
        "IMAGE_TOO_LARGE" => ProviderErrorCode::InputTooLarge,
        _ => ProviderErrorCode::InvalidResponse,
    };
    let user_message = match &error {
        ImageError::Io(_) => "无法读写图片文件，请检查文件路径和权限。",
        ImageError::Encode(_) => "无法保存图片，请检查磁盘空间。",
        _ => error.user_message(),
    };
    ProviderError::new(code, error.to_string()).with_user_message(user_message)
}
