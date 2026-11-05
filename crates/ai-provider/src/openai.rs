use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use image_core::{decode_image_from_memory, encode_atomic, ensure_parent_dir, OutputFormat};
use reqwest::multipart::{Form, Part};
use reqwest::{Response, StatusCode, Url};
use serde_json::Value;
use task_runtime::CancellationToken;

use crate::error::{ProviderError, ProviderErrorCode};
use crate::models::{
    ImageEditProvider, ProgressCallback, ProviderInfo, ProviderOutput, SecretString,
};
use crate::registry::{ProviderRegistry, OPENAI_MAX_INPUT_BYTES};

pub const MAX_API_RESPONSE_BYTES: usize = 80 * 1024 * 1024;
pub const MAX_RESULT_IMAGE_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct OpenAiProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: Option<String>,
    pub allow_localhost: bool,
    pub created_at: String,
    pub credential: SecretString,
}

pub struct OpenAiCompatibleProvider {
    id: String,
    name: String,
    base_url: String,
    model: String,
    allow_localhost: bool,
    created_at: String,
    credential: SecretString,
    client: reqwest::Client,
}

impl std::fmt::Debug for OpenAiCompatibleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleProvider")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("allow_localhost", &self.allow_localhost)
            .field("credential", &self.credential)
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatibleProvider {
    pub fn new(config: OpenAiProviderConfig) -> Result<Self, ProviderError> {
        let url = validate_base_url(&config.base_url, config.allow_localhost)?;
        let local_target = url_is_local(&url);
        let base_url = normalize_base_url(&url);
        let model = config
            .model
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "gpt-image-1".to_string());
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("LumaPortrait/0.1");
        if local_target {
            // System-wide HTTP proxies often do not exempt localhost. A local
            // relay must be reachable directly, otherwise a corporate proxy
            // turns a working local provider into a connection error.
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|e| ProviderError::new(ProviderErrorCode::Network, e.to_string()))?;
        Ok(Self {
            id: config.id,
            name: config.name,
            base_url,
            model,
            allow_localhost: config.allow_localhost,
            created_at: config.created_at,
            credential: config.credential,
            client,
        })
    }

    fn images_edits_url(&self) -> String {
        endpoint(&self.base_url, "/images/edits")
    }

    fn models_url(&self) -> String {
        endpoint(&self.base_url, "/models")
    }

    /// Lightweight authenticated request used by `test_provider`.
    pub async fn ping(&self) -> Result<(), ProviderError> {
        self.validate().await?;
        let response = self
            .client
            .get(self.models_url())
            .bearer_auth(self.credential.expose())
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = read_limited(response, 1024 * 1024, &CancellationToken::new())
            .await
            .unwrap_or_default();
        Err(map_http_error(status, &body))
    }
}

#[async_trait]
impl ImageEditProvider for OpenAiCompatibleProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: crate::ProviderKind::OpenAiCompatible,
            base_url: Some(self.base_url.clone()),
            model: Some(self.model.clone()),
            has_credential: !self.credential.is_empty(),
            capabilities: ProviderRegistry::capabilities_for(crate::ProviderKind::OpenAiCompatible),
            created_at: self.created_at.clone(),
        }
    }

    async fn validate(&self) -> Result<(), ProviderError> {
        let url = validate_base_url(&self.base_url, self.allow_localhost)?;
        if self.model.trim().is_empty() {
            return Err(ProviderError::new(
                ProviderErrorCode::ProviderUnavailable,
                "model is empty",
            )
            .with_user_message("请填写 Provider 模型名称。"));
        }
        if self.credential.is_empty() {
            return Err(ProviderError::new(
                ProviderErrorCode::AuthFailed,
                "credential is missing",
            ));
        }
        ensure_public_resolution(&url, self.allow_localhost).await?;
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        self.validate().await?;
        let response = self
            .client
            .get(self.models_url())
            .bearer_auth(self.credential.expose())
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        let body = read_limited(response, 2 * 1024 * 1024, &CancellationToken::new()).await?;
        if !status.is_success() {
            return Err(map_http_error(status, &body));
        }
        let parsed: Value = serde_json::from_slice(&body).map_err(|error| {
            ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                format!("invalid model list JSON: {error}"),
            )
        })?;
        let mut models: Vec<String> = parsed
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        models.sort();
        models.dedup();
        Ok(models)
    }

    async fn execute(
        &self,
        input_path: &Path,
        prompt: &str,
        strength: f32,
        output_path: &Path,
        progress: ProgressCallback,
        cancel: CancellationToken,
    ) -> Result<ProviderOutput, ProviderError> {
        self.validate().await?;
        cancel.check().map_err(|_| ProviderError::cancelled())?;
        let _ = strength; // Strength is provider-specific; the OpenAI edit API has no field for it.

        let metadata = std::fs::metadata(input_path).map_err(|e| {
            ProviderError::new(ProviderErrorCode::InvalidResponse, e.to_string())
                .with_user_message("无法读取源图片，请重新导入。")
        })?;
        if metadata.len() > OPENAI_MAX_INPUT_BYTES {
            return Err(ProviderError::new(
                ProviderErrorCode::InputTooLarge,
                format!("input file is {} bytes", metadata.len()),
            ));
        }
        let input_bytes = std::fs::read(input_path).map_err(|e| {
            ProviderError::new(ProviderErrorCode::InvalidResponse, e.to_string())
                .with_user_message("无法读取源图片，请重新导入。")
        })?;
        let file_name = input_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input.png")
            .replace(['\r', '\n', '"'], "_");
        let mime = mime_for_path(input_path);
        let part = Part::bytes(input_bytes)
            .file_name(file_name)
            .mime_str(mime)
            .map_err(|e| ProviderError::new(ProviderErrorCode::InvalidResponse, e.to_string()))?;
        let form = Form::new()
            .text("model", self.model.clone())
            .text("prompt", prompt.to_string())
            .text("n", "1")
            .text("size", "auto")
            .text("response_format", "b64_json")
            .part("image", part);

        progress(10);
        let request = self
            .client
            .post(self.images_edits_url())
            .bearer_auth(self.credential.expose())
            .multipart(form);
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
            result = request.send() => result.map_err(map_reqwest_error)?,
        };
        let status = response.status();
        let is_json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_ascii_lowercase().starts_with("application/json"))
            .unwrap_or(false);
        let body = read_limited(response, MAX_API_RESPONSE_BYTES, &cancel).await?;
        if !status.is_success() {
            return Err(map_http_error(status, &body));
        }
        if !is_json {
            return Err(ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                "provider response is not application/json",
            ));
        }
        let parsed: Value = serde_json::from_slice(&body).map_err(|e| {
            ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                format!("invalid provider JSON: {e}"),
            )
        })?;
        if let Some(error) = parsed.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string();
            let mapped = if code.contains("content_policy")
                || message.to_ascii_lowercase().contains("content policy")
            {
                ProviderErrorCode::ContentRejected
            } else {
                ProviderErrorCode::InvalidResponse
            };
            return Err(ProviderError::new(
                mapped,
                format!("provider error {code}: {message}"),
            ));
        }

        progress(80);
        let item = parsed
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .ok_or_else(|| {
                ProviderError::new(
                    ProviderErrorCode::InvalidResponse,
                    "provider response has no data[0]",
                )
            })?;

        let image_bytes = if let Some(encoded) = item.get("b64_json").and_then(Value::as_str) {
            if encoded.len() > MAX_RESULT_IMAGE_BYTES / 3 * 4 + 4 {
                return Err(ProviderError::new(
                    ProviderErrorCode::InvalidResponse,
                    "base64 result is too large",
                ));
            }
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| {
                    ProviderError::new(
                        ProviderErrorCode::InvalidResponse,
                        format!("invalid base64 result: {e}"),
                    )
                })?
        } else if let Some(url) = item.get("url").and_then(Value::as_str) {
            let url = validate_result_url(url, self.allow_localhost)?;
            ensure_public_resolution(&url, self.allow_localhost).await?;
            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
                result = self
                    .client
                    .get(url.clone())
                    .timeout(Duration::from_secs(60))
                    .send() => result.map_err(map_reqwest_error)?,
            };
            let status = response.status();
            if !status.is_success() {
                let body = read_limited(response, 1024 * 1024, &cancel)
                    .await
                    .unwrap_or_default();
                return Err(map_http_error(status, &body));
            }
            let image_content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_ascii_lowercase().starts_with("image/"))
                .unwrap_or(false);
            if !image_content_type {
                return Err(ProviderError::new(
                    ProviderErrorCode::InvalidResponse,
                    "result download is not an image",
                ));
            }
            read_limited(response, MAX_RESULT_IMAGE_BYTES, &cancel).await?
        } else {
            return Err(ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                "provider response has neither b64_json nor url",
            ));
        };

        if image_bytes.len() > MAX_RESULT_IMAGE_BYTES {
            return Err(ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                "result image is too large",
            ));
        }
        cancel.check().map_err(|_| ProviderError::cancelled())?;
        let decoded = decode_image_from_memory(&image_bytes).map_err(|e| {
            ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                format!("invalid result image: {e}"),
            )
        })?;
        ensure_parent_dir(output_path)
            .map_err(|e| ProviderError::new(ProviderErrorCode::InvalidResponse, e.to_string()))?;
        let bytes = encode_atomic(output_path, &decoded, OutputFormat::Png, 100)
            .map_err(|e| ProviderError::new(ProviderErrorCode::InvalidResponse, e.to_string()))?;
        progress(100);
        Ok(ProviderOutput {
            path: output_path.to_path_buf(),
            width: decoded.width,
            height: decoded.height,
            bytes,
        })
    }
}

fn endpoint(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with(path) {
        base.to_string()
    } else {
        format!("{base}{path}")
    }
}

/// Users often paste the full endpoint of an OpenAI-compatible relay rather
/// than the API root. Store the root so `/models` and `/images/edits` are both
/// built correctly.
fn normalize_base_url(url: &Url) -> String {
    let mut base = url.as_str().trim_end_matches('/').to_string();
    for suffix in ["/images/edits", "/images/generations", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    base
}

fn url_is_local(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    let bare = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&host);
    bare.parse::<IpAddr>().map(is_loopback_ip).unwrap_or(false)
}

fn proxy_env_configured() -> bool {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|key| {
            std::env::var_os(key)
                .map(|value| !value.is_empty())
                .unwrap_or(false)
        })
}

/// Resolve remote hostnames and reject private/link-local results. This closes
/// the gap where a public-looking name points at `127.0.0.1` or `10.x`.
/// When an explicit HTTP proxy is configured the proxy resolves the name, so a
/// local DNS preflight would break proxy-only networks; the proxy remains the
/// trust boundary in that case.
async fn ensure_public_resolution(url: &Url, allow_localhost: bool) -> Result<(), ProviderError> {
    let Some(host) = url.host_str() else {
        return Err(invalid_url("地址缺少主机名。"));
    };
    let host = host.to_ascii_lowercase();
    let bare = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&host);
    if bare.parse::<IpAddr>().is_ok() {
        return Ok(()); // already checked synchronously
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return if allow_localhost {
            Ok(())
        } else {
            Err(invalid_url(
                "访问 localhost/回环地址需要在 Provider 配置中显式允许。",
            ))
        };
    }
    if proxy_env_configured() {
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let resolved: Vec<IpAddr> = tokio::net::lookup_host((bare, port))
        .await
        .map_err(|error| {
            ProviderError::new(
                ProviderErrorCode::Network,
                format!("dns resolution failed: {error}"),
            )
            .with_user_message("无法解析 Provider 域名，请检查网络或系统代理。")
        })?
        .map(|address| address.ip())
        .collect();
    if resolved.is_empty() {
        return Err(
            ProviderError::new(ProviderErrorCode::Network, "dns returned no addresses")
                .with_user_message("Provider 域名没有解析结果。"),
        );
    }
    if resolved.iter().any(|ip| is_restricted_ip(*ip)) {
        return Err(invalid_url(
            "Provider 域名解析到了私有或本地地址，已拒绝访问。",
        ));
    }
    Ok(())
}

fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("tif") | Some("tiff") => "image/tiff",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

fn map_reqwest_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::new(ProviderErrorCode::Timeout, error.to_string())
    } else {
        ProviderError::new(ProviderErrorCode::Network, error.to_string())
    }
}

fn extract_api_message(body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("");
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            if !code.is_empty() || !message.is_empty() {
                return format!("{code} {message}").trim().to_string();
            }
        }
    }
    let text = String::from_utf8_lossy(body);
    text.chars().take(300).collect()
}

fn map_http_error(status: StatusCode, body: &[u8]) -> ProviderError {
    let detail = extract_api_message(body);
    let lower = detail.to_ascii_lowercase();
    let (code, message) = match status.as_u16() {
        401 | 403 => (
            ProviderErrorCode::AuthFailed,
            format!("HTTP {}: {detail}", status.as_u16()),
        ),
        429 => (
            ProviderErrorCode::RateLimited,
            format!("HTTP {}: {detail}", status.as_u16()),
        ),
        413 => (
            ProviderErrorCode::InputTooLarge,
            format!("HTTP {}: {detail}", status.as_u16()),
        ),
        400 if lower.contains("content_policy") || lower.contains("content policy") => (
            ProviderErrorCode::ContentRejected,
            format!("HTTP 400: {detail}"),
        ),
        500..=599 => (
            ProviderErrorCode::ProviderUnavailable,
            format!("HTTP {}: {detail}", status.as_u16()),
        ),
        _ => (
            ProviderErrorCode::InvalidResponse,
            format!("HTTP {}: {detail}", status.as_u16()),
        ),
    };
    ProviderError::new(code, message)
}

async fn read_limited(
    mut response: Response,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, ProviderError> {
    if let Some(length) = response.content_length() {
        if length > limit as u64 {
            return Err(ProviderError::new(
                ProviderErrorCode::InvalidResponse,
                format!("response body is too large: {length} bytes"),
            ));
        }
    }
    let mut body = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::cancelled()),
            chunk = response.chunk() => chunk.map_err(map_reqwest_error)?,
        };
        match chunk {
            Some(chunk) => {
                if body.len() + chunk.len() > limit {
                    return Err(ProviderError::new(
                        ProviderErrorCode::InvalidResponse,
                        "response body exceeded size limit",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            None => break,
        }
    }
    Ok(body)
}

/// HTTPS anywhere public. HTTP only for loopback with explicit opt-in. Private,
/// link-local and unspecified IP literals are always rejected.
pub fn validate_base_url(raw: &str, allow_localhost: bool) -> Result<Url, ProviderError> {
    validate_url(raw, allow_localhost, false)
}

pub fn validate_result_url(raw: &str, allow_localhost: bool) -> Result<Url, ProviderError> {
    validate_url(raw, allow_localhost, true)
}

fn validate_url(raw: &str, allow_localhost: bool, allow_query: bool) -> Result<Url, ProviderError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_url("Provider 地址不能为空。"));
    }
    let url = Url::parse(raw).map_err(|e| {
        ProviderError::new(
            ProviderErrorCode::ProviderUnavailable,
            format!("invalid provider URL: {e}"),
        )
        .with_user_message("Provider 地址无效，请填写完整的 HTTPS 地址。")
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid_url("地址中不能包含用户名或密码。"));
    }
    if url.fragment().is_some() {
        return Err(invalid_url("地址不能包含片段（#）。"));
    }
    if !allow_query && url.query().is_some() {
        return Err(invalid_url("Provider 基础地址不能包含查询参数。"));
    }
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(invalid_url("仅支持 HTTP/HTTPS 地址。"));
    }
    let host = url
        .host_str()
        .map(|host| host.to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .ok_or_else(|| invalid_url("地址缺少主机名。"))?;
    // `Url::host_str` keeps the brackets around IPv6 literals (`[::1]`).
    let ip_literal = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&host);
    let ip = ip_literal.parse::<IpAddr>().ok();
    let localhost_name = host == "localhost" || host.ends_with(".localhost");
    let loopback = ip.map(is_loopback_ip).unwrap_or(false);
    let local = localhost_name || loopback;

    if let Some(ip) = ip {
        if is_restricted_ip(ip) {
            return Err(invalid_url("不允许访问私有或本地网络地址。"));
        }
    }
    if host.ends_with(".local") || host.ends_with(".internal") {
        return Err(invalid_url("不允许访问本地域名。"));
    }
    if local && !allow_localhost {
        return Err(invalid_url(
            "访问 localhost/回环地址需要在 Provider 配置中显式允许。",
        ));
    }
    if scheme == "http" && !(local && allow_localhost) {
        return Err(invalid_url("远程 Provider 地址必须使用 HTTPS。"));
    }
    Ok(url)
}

fn invalid_url(user_message: &str) -> ProviderError {
    ProviderError::new(
        ProviderErrorCode::ProviderUnavailable,
        format!("provider URL rejected: {user_message}"),
    )
    .with_user_message(user_message)
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6
                    .to_ipv4_mapped()
                    .map(|v4| v4.is_loopback())
                    .unwrap_or(false)
        }
    }
}

fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && (18..=19).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        }
        IpAddr::V6(v6) => {
            v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .map(|v4| is_restricted_ip(IpAddr::V4(v4)))
                    .unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SecretString;

    #[test]
    fn validates_public_https_urls() {
        assert!(validate_base_url("https://api.openai.com/v1", false).is_ok());
        assert!(validate_base_url("https://example.com:8443/v1", false).is_ok());
    }

    #[test]
    fn rejects_plain_http_and_private_targets() {
        assert!(validate_base_url("http://api.openai.com/v1", false).is_err());
        assert!(validate_base_url("http://localhost:8080/v1", false).is_err());
        assert!(validate_base_url("http://127.0.0.1:8080/v1", true).is_ok());
        assert!(validate_base_url("https://127.0.0.1/v1", true).is_ok());
        assert!(validate_base_url("https://10.0.0.1/v1", true).is_err());
        assert!(validate_base_url("https://169.254.169.254/latest", true).is_err());
        assert!(validate_base_url("https://192.168.1.2/v1", true).is_err());
        assert!(validate_base_url("https://[::1]/v1", false).is_err());
        assert!(validate_base_url("https://[::1]/v1", true).is_ok());
        assert!(validate_base_url("https://[fe80::1]/v1", true).is_err());
        assert!(validate_base_url("https://service.local/v1", true).is_err());
    }

    #[test]
    fn rejects_credentials_and_odd_schemes() {
        assert!(validate_base_url("https://user:pass@api.openai.com/v1", false).is_err());
        assert!(validate_base_url("ftp://api.openai.com/v1", false).is_err());
        assert!(validate_base_url("file:///etc/passwd", false).is_err());
        assert!(validate_base_url("", false).is_err());
        assert!(validate_base_url("https://api.openai.com/v1?x=1", false).is_err());
    }

    #[test]
    fn result_urls_can_have_query_parameters() {
        assert!(validate_result_url("https://cdn.example.com/a.png?token=abc", false).is_ok());
        assert!(validate_result_url("http://127.0.0.1:9000/a.png", true).is_ok());
        assert!(validate_result_url("http://127.0.0.1:9000/a.png", false).is_err());
    }

    #[test]
    fn normalizes_full_endpoint_proxy_urls() {
        let make = |base_url: &str| {
            OpenAiCompatibleProvider::new(OpenAiProviderConfig {
                id: "p".into(),
                name: "Relay".into(),
                base_url: base_url.into(),
                model: Some("gpt-image-1".into()),
                allow_localhost: false,
                created_at: "2026-01-01T00:00:00.000Z".into(),
                credential: SecretString::new("key"),
            })
            .unwrap()
        };
        let edits = make("https://relay.example.com/v1/images/edits/");
        assert_eq!(edits.base_url, "https://relay.example.com/v1");
        assert_eq!(
            edits.images_edits_url(),
            "https://relay.example.com/v1/images/edits"
        );
        assert_eq!(edits.models_url(), "https://relay.example.com/v1/models");

        let models = make("https://relay.example.com/v1/models");
        assert_eq!(models.base_url, "https://relay.example.com/v1");
        assert_eq!(
            models.images_edits_url(),
            "https://relay.example.com/v1/images/edits"
        );
    }

    #[test]
    fn only_real_loopback_hosts_are_local() {
        let parse = |value: &str| Url::parse(value).unwrap();
        assert!(url_is_local(&parse("http://localhost:8080")));
        assert!(url_is_local(&parse("http://127.0.0.2:8080")));
        assert!(url_is_local(&parse("http://[::1]:8080")));
        assert!(!url_is_local(&parse("https://localhost.evil.com/v1")));
        assert!(!url_is_local(&parse("https://api.openai.com/v1")));
    }

    #[test]
    fn provider_debug_redacts_secret() {
        let provider = OpenAiCompatibleProvider::new(OpenAiProviderConfig {
            id: "p".into(),
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: Some("gpt-image-1".into()),
            allow_localhost: false,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            credential: SecretString::new("super-secret-key"),
        })
        .unwrap();
        assert!(!format!("{provider:?}").contains("super-secret-key"));
        assert_eq!(
            provider.images_edits_url(),
            "https://api.openai.com/v1/images/edits"
        );
        assert_eq!(provider.models_url(), "https://api.openai.com/v1/models");
    }

    #[test]
    fn missing_credential_is_reported() {
        let provider = OpenAiCompatibleProvider::new(OpenAiProviderConfig {
            id: "p".into(),
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: Some("gpt-image-1".into()),
            allow_localhost: false,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            credential: SecretString::new(""),
        })
        .unwrap();
        let err = futures_lite_block_on(provider.validate()).unwrap_err();
        assert_eq!(err.code, ProviderErrorCode::AuthFailed);
    }

    fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }
}
