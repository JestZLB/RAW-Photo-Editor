use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use thiserror::Error;

pub const KEYRING_SERVICE: &str = "com.lumaportrait.editor";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CredentialError {
    #[error("credential platform error: {0}")]
    Platform(String),
    #[error("credential store lock is poisoned")]
    LockPoisoned,
}

/// Secret store abstraction. Implementations must never log or serialize the
/// secret. SQLite/provider rows only store configuration, never API keys.
pub trait CredentialStore: Send + Sync {
    fn set(&self, provider_id: &str, secret: &str) -> Result<(), CredentialError>;
    fn get(&self, provider_id: &str) -> Result<Option<String>, CredentialError>;
    fn delete(&self, provider_id: &str) -> Result<(), CredentialError>;

    fn has(&self, provider_id: &str) -> bool {
        matches!(self.get(provider_id), Ok(Some(value)) if !value.is_empty())
    }
}

#[derive(Default)]
pub struct MemoryCredentialStore {
    values: Mutex<HashMap<String, String>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MemoryCredentialStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryCredentialStore")
            .finish_non_exhaustive()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn set(&self, provider_id: &str, secret: &str) -> Result<(), CredentialError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| CredentialError::LockPoisoned)?;
        values.insert(provider_id.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, provider_id: &str) -> Result<Option<String>, CredentialError> {
        let values = self
            .values
            .lock()
            .map_err(|_| CredentialError::LockPoisoned)?;
        Ok(values.get(provider_id).cloned())
    }

    fn delete(&self, provider_id: &str) -> Result<(), CredentialError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| CredentialError::LockPoisoned)?;
        values.remove(provider_id);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsCredentialStore;

#[cfg(target_os = "windows")]
impl WindowsCredentialStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(provider_id: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(KEYRING_SERVICE, provider_id)
            .map_err(|e| CredentialError::Platform(e.to_string()))
    }
}

#[cfg(target_os = "windows")]
impl CredentialStore for WindowsCredentialStore {
    fn set(&self, provider_id: &str, secret: &str) -> Result<(), CredentialError> {
        Self::entry(provider_id)?
            .set_password(secret)
            .map_err(|e| CredentialError::Platform(e.to_string()))
    }

    fn get(&self, provider_id: &str) -> Result<Option<String>, CredentialError> {
        match Self::entry(provider_id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CredentialError::Platform(e.to_string())),
        }
    }

    fn delete(&self, provider_id: &str) -> Result<(), CredentialError> {
        match Self::entry(provider_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Platform(e.to_string())),
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub type WindowsCredentialStore = MemoryCredentialStore;

/// Windows uses Credential Manager. Other platforms compile against the
/// in-memory fallback so development/tests do not require OS keychain access.
pub fn default_credential_store() -> Arc<dyn CredentialStore> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(WindowsCredentialStore::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(MemoryCredentialStore::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryCredentialStore::new();
        assert!(!store.has("provider"));
        store.set("provider", "secret-value").unwrap();
        assert!(store.has("provider"));
        assert_eq!(
            store.get("provider").unwrap().as_deref(),
            Some("secret-value")
        );
        store.delete("provider").unwrap();
        assert_eq!(store.get("provider").unwrap(), None);
        let debug = format!("{store:?}");
        assert!(!debug.contains("secret-value"));
    }
}
