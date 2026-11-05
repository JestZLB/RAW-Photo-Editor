//! SQLite metadata store, project asset/cache layout and credential storage.

mod assets;
mod credentials;
mod db;

pub use assets::{sanitize_file_name, AssetStore};
pub use credentials::{
    default_credential_store, CredentialError, CredentialStore, MemoryCredentialStore,
    WindowsCredentialStore, KEYRING_SERVICE,
};
pub use db::{
    AiJobRecord, AssetRecord, Database, PersistenceError, ProjectRecord, ProjectSummaryRecord,
    ProviderRecord,
};
