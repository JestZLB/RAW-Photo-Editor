use std::collections::HashMap;
use std::sync::RwLock;

use crate::models::{now_rfc3339, ProviderCapabilities, ProviderInfo, ProviderKind};

pub const MOCK_PROVIDER_ID: &str = "local-mock-provider";
pub const MOCK_PROVIDER_NAME: &str = "本地演示 Provider";
pub const OPENAI_MAX_INPUT_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub allow_localhost: bool,
    pub created_at: String,
}

impl StoredProviderConfig {
    pub fn mock(created_at: impl Into<String>) -> Self {
        Self {
            id: MOCK_PROVIDER_ID.to_string(),
            name: MOCK_PROVIDER_NAME.to_string(),
            kind: ProviderKind::Mock,
            base_url: None,
            model: None,
            allow_localhost: false,
            created_at: created_at.into(),
        }
    }
}

#[derive(Debug)]
pub struct ProviderRegistry {
    entries: RwLock<HashMap<String, StoredProviderConfig>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Registry used by the app on first start: mock only, no secrets.
    pub fn seeded() -> Self {
        let registry = Self::new();
        registry.upsert(StoredProviderConfig::mock(now_rfc3339()));
        registry
    }

    pub fn upsert(&self, config: StoredProviderConfig) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(config.id.clone(), config);
        }
    }

    pub fn get(&self, id: &str) -> Option<StoredProviderConfig> {
        self.entries
            .read()
            .ok()
            .and_then(|entries| entries.get(id).cloned())
    }

    pub fn list(&self) -> Vec<StoredProviderConfig> {
        let mut values = self
            .entries
            .read()
            .map(|entries| entries.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        values.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        values
    }

    pub fn len(&self) -> usize {
        self.entries
            .read()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capabilities_for(kind: ProviderKind) -> ProviderCapabilities {
        match kind {
            // The mock runs locally, never uploads, and can cancel between
            // deterministic processing steps.
            ProviderKind::Mock => ProviderCapabilities {
                supports_mask: false,
                supports_reference_images: false,
                supports_multiple_images: false,
                returns_image: true,
                supports_structured_parameters: false,
                supports_cancellation: true,
                max_width: None,
                max_height: None,
                max_input_bytes: None,
            },
            // We do not send a mask today, so capabilities report it honestly.
            ProviderKind::OpenAiCompatible => ProviderCapabilities {
                supports_mask: false,
                supports_reference_images: false,
                supports_multiple_images: false,
                returns_image: true,
                supports_structured_parameters: false,
                supports_cancellation: true,
                max_width: None,
                max_height: None,
                max_input_bytes: Some(OPENAI_MAX_INPUT_BYTES),
            },
        }
    }

    pub fn info_for(config: &StoredProviderConfig, has_credential: bool) -> ProviderInfo {
        ProviderInfo {
            id: config.id.clone(),
            name: config.name.clone(),
            kind: config.kind,
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            has_credential,
            capabilities: Self::capabilities_for(config.kind),
            created_at: config.created_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_registry_contains_only_mock() {
        let registry = ProviderRegistry::seeded();
        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, MOCK_PROVIDER_ID);
        assert_eq!(list[0].kind, ProviderKind::Mock);
        let info = ProviderRegistry::info_for(&list[0], false);
        assert!(info.capabilities.returns_image);
        assert!(!info.capabilities.supports_mask);
        assert_eq!(
            ProviderRegistry::capabilities_for(ProviderKind::OpenAiCompatible).max_input_bytes,
            Some(OPENAI_MAX_INPUT_BYTES)
        );
    }
}
