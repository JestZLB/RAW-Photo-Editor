use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorCode {
    ProviderUnavailable,
    AuthFailed,
    RateLimited,
    Timeout,
    InvalidResponse,
    ContentRejected,
    Network,
    Cancelled,
    InputTooLarge,
}

impl ProviderErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "PROVIDER_UNAVAILABLE",
            Self::AuthFailed => "AUTH_FAILED",
            Self::RateLimited => "RATE_LIMITED",
            Self::Timeout => "TIMEOUT",
            Self::InvalidResponse => "INVALID_RESPONSE",
            Self::ContentRejected => "CONTENT_REJECTED",
            Self::Network => "NETWORK",
            Self::Cancelled => "CANCELLED",
            Self::InputTooLarge => "INPUT_TOO_LARGE",
        }
    }

    fn default_user_message(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "AI 服务暂时不可用，请稍后重试。",
            Self::AuthFailed => "API Key 无效或已过期，请检查 Provider 配置。",
            Self::RateLimited => "请求过于频繁，请稍后再试。",
            Self::Timeout => "AI 服务响应超时，请稍后重试。",
            Self::InvalidResponse => "AI 服务返回了无法解析的结果。",
            Self::ContentRejected => "AI 服务拒绝了本次请求，请调整提示词或图片。",
            Self::Network => "网络连接失败，请检查网络或 Provider 地址。",
            Self::Cancelled => "任务已取消。",
            Self::InputTooLarge => "输入图片过大，请压缩后重试。",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderError {
    pub code: ProviderErrorCode,
    pub message: String,
    pub user_message: String,
}

impl ProviderError {
    pub fn new(code: ProviderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            user_message: code.default_user_message().to_string(),
        }
    }

    pub fn with_user_message(mut self, user_message: impl Into<String>) -> Self {
        self.user_message = user_message.into();
        self
    }

    pub fn cancelled() -> Self {
        Self::new(ProviderErrorCode::Cancelled, "cancelled by user")
    }

    pub fn is_cancelled(&self) -> bool {
        self.code == ProviderErrorCode::Cancelled
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ProviderError {}
