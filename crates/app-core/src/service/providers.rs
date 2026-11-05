use std::time::{Duration, Instant};

use ai_provider::{
    now_rfc3339, validate_base_url, OpenAiCompatibleProvider, OpenAiProviderConfig,
    ProviderConfigInput, ProviderInfo, ProviderKind, ProviderTestResult, SecretString,
    StoredProviderConfig, MOCK_PROVIDER_ID,
};
use persistence::ProviderRecord;
use uuid::Uuid;

use super::AppService;
use crate::error::ApiError;
use crate::requests::{SetProviderCredentialRequest, TestProviderRequest};

impl AppService {
    pub fn list_providers(&self) -> Result<Vec<ProviderInfo>, ApiError> {
        let records = self.db.list_providers()?;
        let mut providers = Vec::with_capacity(records.len());
        for record in records {
            providers.push(self.provider_info(&record)?);
        }
        Ok(providers)
    }

    pub fn save_provider_config(
        &self,
        input: ProviderConfigInput,
    ) -> Result<ProviderInfo, ApiError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(ApiError::invalid_argument(
                "provider name is empty",
                "请填写 Provider 名称。",
            ));
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
        if name.trim().is_empty() {
            return Err(ApiError::invalid_argument(
                "provider name has no printable characters",
                "请填写 Provider 名称。",
            ));
        }
        let existing_id = input
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if existing_id.as_deref() == Some(MOCK_PROVIDER_ID) && input.kind != ProviderKind::Mock {
            return Err(ApiError::invalid_argument(
                "built-in mock provider cannot change kind",
                "内置演示 Provider 不能更改为其他类型。",
            ));
        }
        if let Some(id) = existing_id.as_deref() {
            // Reject credentials embedded in ids/URLs early, without echoing.
            if id.len() > 128 || id.chars().any(char::is_control) {
                return Err(ApiError::invalid_argument(
                    "invalid provider id",
                    "Provider 标识无效。",
                ));
            }
        }
        let id = existing_id.unwrap_or_else(|| {
            if input.kind == ProviderKind::Mock {
                format!("mock-{}", Uuid::new_v4())
            } else {
                Uuid::new_v4().to_string()
            }
        });

        let allow_localhost = input.allow_localhost.unwrap_or(false);
        let (base_url, model) = match input.kind {
            ProviderKind::Mock => (None, None),
            ProviderKind::OpenAiCompatible => {
                let base_url = input.base_url.unwrap_or_default().trim().to_string();
                if base_url.is_empty() {
                    return Err(ApiError::invalid_argument(
                        "base url is empty",
                        "请填写 OpenAI 兼容服务地址。",
                    ));
                }
                // SSRF hardening: only HTTPS, except explicit loopback HTTP.
                validate_base_url(&base_url, allow_localhost)?;
                let model = input
                    .model
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                (Some(base_url), model)
            }
        };

        let existing = self.db.get_provider(&id)?;
        let created_at = existing
            .as_ref()
            .map(|record| record.created_at.clone())
            .unwrap_or_else(now_rfc3339);
        let record = ProviderRecord {
            id: id.clone(),
            name,
            kind: input.kind.as_str().to_string(),
            base_url,
            model,
            allow_localhost,
            created_at,
        };
        self.db.upsert_provider(&record)?;
        self.providers.upsert(StoredProviderConfig {
            id: record.id.clone(),
            name: record.name.clone(),
            kind: input.kind,
            base_url: record.base_url.clone(),
            model: record.model.clone(),
            allow_localhost: record.allow_localhost,
            created_at: record.created_at.clone(),
        });
        self.provider_info(&record)
    }

    pub fn set_provider_credential(
        &self,
        request: SetProviderCredentialRequest,
    ) -> Result<ProviderInfo, ApiError> {
        let record = self.provider_record(request.provider_id.trim())?;
        let kind = ProviderKind::from_label(&record.kind)
            .ok_or_else(|| ApiError::internal("unknown provider kind in database"))?;
        if kind == ProviderKind::Mock {
            return Err(ApiError::invalid_argument(
                "mock provider does not use credentials",
                "本地演示 Provider 无需 API Key。",
            ));
        }
        let key = request.api_key.trim();
        if key.len() > 8192 || key.chars().any(char::is_control) {
            return Err(ApiError::invalid_argument(
                "invalid credential format",
                "API Key 格式无效。",
            ));
        }
        if key.is_empty() {
            self.credentials.delete(&record.id)?;
        } else {
            self.credentials.set(&record.id, key)?;
        }
        let record = self.provider_record(&record.id)?;
        self.provider_info(&record)
    }

    pub async fn test_provider(
        &self,
        request: TestProviderRequest,
    ) -> Result<ProviderTestResult, ApiError> {
        let record = self.provider_record(request.provider_id.trim())?;
        let kind = ProviderKind::from_label(&record.kind)
            .ok_or_else(|| ApiError::internal("unknown provider kind in database"))?;
        if kind == ProviderKind::Mock {
            return Ok(ProviderTestResult {
                ok: true,
                latency_ms: 0,
                message: "本地演示 Provider 可用，图片不会上传。".to_string(),
            });
        }

        let started = Instant::now();
        let Some(base_url) = record
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(provider_test_failure(
                started,
                "请先填写 OpenAI 兼容服务地址。",
            ));
        };
        let credential = match self.credentials.get(&record.id)? {
            Some(value) if !value.trim().is_empty() => value,
            _ => {
                return Ok(provider_test_failure(
                    started,
                    "尚未设置 API Key，请先保存凭据。",
                ))
            }
        };
        let config = OpenAiProviderConfig {
            id: record.id.clone(),
            name: record.name.clone(),
            base_url: base_url.to_string(),
            model: record.model.clone(),
            allow_localhost: record.allow_localhost,
            created_at: record.created_at.clone(),
            credential: SecretString::new(credential),
        };
        let provider = match OpenAiCompatibleProvider::new(config) {
            Ok(provider) => provider,
            Err(error) => {
                return Ok(provider_test_failure(started, &error.user_message));
            }
        };
        match tokio::time::timeout(Duration::from_secs(12), provider.ping()).await {
            Ok(Ok(())) => Ok(ProviderTestResult {
                ok: true,
                latency_ms: started.elapsed().as_millis() as u64,
                message: "连接成功。".to_string(),
            }),
            Ok(Err(error)) => Ok(provider_test_failure(started, &error.user_message)),
            Err(_) => Ok(provider_test_failure(
                started,
                "连接超时，请检查网络或服务地址。",
            )),
        }
    }
}

fn provider_test_failure(started: Instant, message: &str) -> ProviderTestResult {
    ProviderTestResult {
        ok: false,
        latency_ms: started.elapsed().as_millis() as u64,
        message: message.to_string(),
    }
}
