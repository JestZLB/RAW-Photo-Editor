use ai_provider::ProviderError;
use document::DocumentError;
use image_core::ImageError;
use persistence::{CredentialError, PersistenceError};
use raw_develop::RawError;
use render_engine::RenderError;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Error)]
#[serde(rename_all = "camelCase")]
#[error("{code}: {message}")]
pub struct ApiError {
    /// Stable error code.
    pub code: String,
    /// Internal diagnostic message (safe to log, never contains secrets).
    pub message: String,
    /// Chinese message safe to show in the UI.
    pub user_message: String,
}

impl ApiError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        user_message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            user_message: user_message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>, user_message: impl Into<String>) -> Self {
        Self::new("INVALID_ARGUMENT", message, user_message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL", message, "应用出现内部错误，请重试。")
    }

    pub fn project_not_found() -> Self {
        Self::new(
            "PROJECT_NOT_FOUND",
            "project not found",
            "项目不存在或已被移除。",
        )
    }

    pub fn source_missing() -> Self {
        Self::new(
            "SOURCE_MISSING",
            "source image is missing",
            "源图片文件缺失，请确认原图未被移动或删除。",
        )
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::new(
            "IO",
            error.to_string(),
            "无法读写本地文件，请检查磁盘空间或权限。",
        )
    }
}

impl From<ImageError> for ApiError {
    fn from(error: ImageError) -> Self {
        Self::new(error.code(), error.to_string(), error.user_message())
    }
}

impl From<DocumentError> for ApiError {
    fn from(error: DocumentError) -> Self {
        Self::new(error.code(), error.to_string(), error.user_message())
    }
}

impl From<RenderError> for ApiError {
    fn from(error: RenderError) -> Self {
        Self::new(error.code(), error.to_string(), error.user_message())
    }
}

impl From<RawError> for ApiError {
    fn from(error: RawError) -> Self {
        Self::new(error.code(), error.to_string(), error.user_message())
    }
}

impl From<ProviderError> for ApiError {
    fn from(error: ProviderError) -> Self {
        Self::new(error.code_str(), error.to_string(), error.user_message)
    }
}

impl From<PersistenceError> for ApiError {
    fn from(error: PersistenceError) -> Self {
        let user_message = match &error {
            PersistenceError::UnsupportedSchema(_) => "项目数据版本过新，请升级应用后再打开。",
            PersistenceError::Io(_) => "无法读写本地项目数据，请检查磁盘空间或权限。",
            _ => "本地数据库操作失败，请重试。",
        };
        Self::new(error.code(), error.to_string(), user_message)
    }
}

impl From<CredentialError> for ApiError {
    fn from(error: CredentialError) -> Self {
        Self::new(
            "CREDENTIAL_ERROR",
            error.to_string(),
            "无法访问系统凭据存储，请检查 Windows 凭据管理器权限。",
        )
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(
            "INVALID_PROJECT_DATA",
            format!("project JSON error: {error}"),
            "项目数据格式无效，无法打开。",
        )
    }
}
