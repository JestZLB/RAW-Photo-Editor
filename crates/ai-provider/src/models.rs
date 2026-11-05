use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use task_runtime::CancellationToken;

use crate::ProviderError;

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Mock,
    OpenAiCompatible,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::OpenAiCompatible => "openaiCompatible",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value {
            "mock" => Some(Self::Mock),
            "openaiCompatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub supports_mask: bool,
    pub supports_reference_images: bool,
    pub supports_multiple_images: bool,
    pub returns_image: bool,
    pub supports_structured_parameters: bool,
    pub supports_cancellation: bool,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub max_input_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub has_credential: bool,
    pub capabilities: ProviderCapabilities,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfigInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub allow_localhost: Option<bool>,
}

impl Default for ProviderConfigInput {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            kind: ProviderKind::Mock,
            base_url: None,
            model: None,
            allow_localhost: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiJobOperation {
    Edit,
    Suggest,
}

impl AiJobOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Suggest => "suggest",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value {
            "suggest" => Self::Suggest,
            _ => Self::Edit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl AiJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiJobInfo {
    pub id: String,
    pub project_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub operation: AiJobOperation,
    pub prompt: String,
    pub status: AiJobStatus,
    pub progress: Option<f32>,
    pub message: Option<String>,
    /// Asset id of the stored result. Applied-state checks compare this with
    /// `LayerState.assetId`; it differs from the job id.
    pub result_asset_id: Option<String>,
    pub result_path: Option<String>,
    pub error_code: Option<String>,
    pub user_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct AiJobRequest {
    pub job_id: Uuid,
    pub project_id: Uuid,
    pub provider_id: String,
    pub operation: AiJobOperation,
    pub prompt: String,
    pub strength: f32,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderOutput {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

pub type ProgressCallback = Arc<dyn Fn(u8) + Send + Sync>;

/// Secret wrapper that redacts itself from Debug output. Never log the inner
/// value; only the provider client calls `expose`.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

#[async_trait::async_trait]
pub trait ImageEditProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    async fn validate(&self) -> Result<(), ProviderError>;
    async fn list_models(&self) -> Result<Vec<String>, ProviderError>;
    async fn execute(
        &self,
        input_path: &std::path::Path,
        prompt: &str,
        strength: f32,
        output_path: &std::path::Path,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, ProviderError>;
}
