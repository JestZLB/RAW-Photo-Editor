use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension, Row};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    collection_id TEXT NOT NULL DEFAULT '',
    collection_name TEXT NOT NULL DEFAULT '',
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    document_json TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_assets_project ON assets(project_id);
CREATE TABLE IF NOT EXISTS ai_jobs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    provider_name TEXT NOT NULL,
    operation TEXT NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,
    progress REAL,
    message TEXT,
    result_asset_id TEXT,
    result_path TEXT,
    error_code TEXT,
    user_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_ai_jobs_project ON ai_jobs(project_id);
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    base_url TEXT,
    model TEXT,
    allow_localhost INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
"#;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database lock is poisoned")]
    LockPoisoned,
    #[error("unsupported database schema version {0}")]
    UnsupportedSchema(i64),
}

impl PersistenceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "DATABASE_ERROR",
            Self::Io(_) => "IO",
            Self::LockPoisoned => "DATABASE_ERROR",
            Self::UnsupportedSchema(_) => "UNSUPPORTED_SCHEMA",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: Uuid,
    pub name: String,
    pub collection_id: Uuid,
    pub collection_name: String,
    pub width: u32,
    pub height: u32,
    pub document_json: String,
    pub schema_version: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummaryRecord {
    pub id: Uuid,
    pub name: String,
    pub collection_id: Uuid,
    pub collection_name: String,
    pub width: u32,
    pub height: u32,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiJobRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider_id: String,
    pub provider_name: String,
    pub operation: String,
    pub prompt: String,
    pub status: String,
    pub progress: Option<f32>,
    pub message: Option<String>,
    pub result_asset_id: Option<Uuid>,
    pub result_path: Option<String>,
    pub error_code: Option<String>,
    pub user_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub allow_localhost: bool,
    pub created_at: String,
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, PersistenceError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(PersistenceError::UnsupportedSchema(version));
        }
        if version < SCHEMA_VERSION {
            conn.execute_batch(SCHEMA)?;
            if !has_column(&conn, "projects", "collection_id")? {
                conn.execute_batch(
                    "ALTER TABLE projects ADD COLUMN collection_id TEXT NOT NULL DEFAULT '';
                     ALTER TABLE projects ADD COLUMN collection_name TEXT NOT NULL DEFAULT '';",
                )?;
            }
            // Every pre-collection project becomes its own single-photo set.
            conn.execute(
                "UPDATE projects SET collection_id = id, collection_name = name
                 WHERE collection_id = ''",
                [],
            )?;
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_projects_collection ON projects(collection_id);",
            )?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, PersistenceError> {
        self.conn.lock().map_err(|_| PersistenceError::LockPoisoned)
    }

    pub fn collection_name_for(
        &self,
        collection_id: Uuid,
    ) -> Result<Option<String>, PersistenceError> {
        let conn = self.connection()?;
        let name = conn
            .query_row(
                "SELECT collection_name FROM projects WHERE collection_id = ?1 LIMIT 1",
                params![collection_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(name)
    }

    pub fn save_project(&self, project: &ProjectRecord) -> Result<(), PersistenceError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO projects (id, name, collection_id, collection_name, width, height,
                document_json, schema_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                collection_id = excluded.collection_id,
                collection_name = excluded.collection_name,
                width = excluded.width,
                height = excluded.height,
                document_json = excluded.document_json,
                schema_version = excluded.schema_version,
                updated_at = excluded.updated_at",
            params![
                project.id.to_string(),
                project.name,
                project.collection_id.to_string(),
                project.collection_name,
                project.width as i64,
                project.height as i64,
                project.document_json,
                project.schema_version as i64,
                project.created_at,
                project.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_project(&self, id: Uuid) -> Result<Option<ProjectRecord>, PersistenceError> {
        let conn = self.connection()?;
        let record = conn
            .query_row(
                "SELECT id, name, collection_id, collection_name, width, height,
                        document_json, schema_version, created_at, updated_at
                 FROM projects WHERE id = ?1",
                params![id.to_string()],
                map_project,
            )
            .optional()?;
        Ok(record)
    }

    pub fn delete_project(&self, id: Uuid) -> Result<bool, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "DELETE FROM projects WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(changed > 0)
    }

    pub fn list_project_summaries(&self) -> Result<Vec<ProjectSummaryRecord>, PersistenceError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, collection_id, collection_name, width, height, updated_at
             FROM projects ORDER BY updated_at DESC LIMIT 500",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectSummaryRecord {
                id: get_uuid(row, 0)?,
                name: row.get(1)?,
                collection_id: get_uuid(row, 2)?,
                collection_name: row.get(3)?,
                width: row.get::<_, i64>(4)? as u32,
                height: row.get::<_, i64>(5)? as u32,
                updated_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn insert_asset(&self, asset: &AssetRecord) -> Result<(), PersistenceError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO assets (id, project_id, kind, path, width, height, bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                path = excluded.path,
                width = excluded.width,
                height = excluded.height,
                bytes = excluded.bytes",
            params![
                asset.id.to_string(),
                asset.project_id.to_string(),
                asset.kind,
                asset.path,
                asset.width as i64,
                asset.height as i64,
                asset.bytes as i64,
                asset.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_asset(&self, id: Uuid) -> Result<(), PersistenceError> {
        let conn = self.connection()?;
        conn.execute("DELETE FROM assets WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    pub fn get_asset(&self, id: Uuid) -> Result<Option<AssetRecord>, PersistenceError> {
        let conn = self.connection()?;
        let record = conn
            .query_row(
                "SELECT id, project_id, kind, path, width, height, bytes, created_at
                 FROM assets WHERE id = ?1",
                params![id.to_string()],
                map_asset,
            )
            .optional()?;
        Ok(record)
    }

    pub fn list_assets(&self, project_id: Uuid) -> Result<Vec<AssetRecord>, PersistenceError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, kind, path, width, height, bytes, created_at
             FROM assets WHERE project_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![project_id.to_string()], map_asset)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_source_asset(
        &self,
        project_id: Uuid,
    ) -> Result<Option<AssetRecord>, PersistenceError> {
        let conn = self.connection()?;
        let record = conn
            .query_row(
                "SELECT id, project_id, kind, path, width, height, bytes, created_at
                 FROM assets WHERE project_id = ?1 AND kind = 'source'
                 ORDER BY created_at ASC LIMIT 1",
                params![project_id.to_string()],
                map_asset,
            )
            .optional()?;
        Ok(record)
    }

    pub fn upsert_provider(&self, provider: &ProviderRecord) -> Result<(), PersistenceError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, model, allow_localhost, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                base_url = excluded.base_url,
                model = excluded.model,
                allow_localhost = excluded.allow_localhost",
            params![
                provider.id,
                provider.name,
                provider.kind,
                provider.base_url,
                provider.model,
                provider.allow_localhost as i64,
                provider.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<ProviderRecord>, PersistenceError> {
        let conn = self.connection()?;
        let record = conn
            .query_row(
                "SELECT id, name, kind, base_url, model, allow_localhost, created_at
                 FROM providers WHERE id = ?1",
                params![id],
                map_provider,
            )
            .optional()?;
        Ok(record)
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderRecord>, PersistenceError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, base_url, model, allow_localhost, created_at
             FROM providers ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], map_provider)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn upsert_ai_job(&self, job: &AiJobRecord) -> Result<(), PersistenceError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO ai_jobs (id, project_id, provider_id, provider_name, operation, prompt,
                status, progress, message, result_asset_id, result_path, error_code, user_message,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                provider_id = excluded.provider_id,
                provider_name = excluded.provider_name,
                operation = excluded.operation,
                prompt = excluded.prompt,
                status = excluded.status,
                progress = excluded.progress,
                message = excluded.message,
                result_asset_id = excluded.result_asset_id,
                result_path = excluded.result_path,
                error_code = excluded.error_code,
                user_message = excluded.user_message,
                updated_at = excluded.updated_at",
            params![
                job.id.to_string(),
                job.project_id.to_string(),
                job.provider_id,
                job.provider_name,
                job.operation,
                job.prompt,
                job.status,
                job.progress.map(|v| v as f64),
                job.message,
                job.result_asset_id.map(|id| id.to_string()),
                job.result_path,
                job.error_code,
                job.user_message,
                job.created_at,
                job.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_ai_job(&self, id: Uuid) -> Result<Option<AiJobRecord>, PersistenceError> {
        let conn = self.connection()?;
        let record = conn
            .query_row(
                "SELECT id, project_id, provider_id, provider_name, operation, prompt, status,
                    progress, message, result_asset_id, result_path, error_code, user_message,
                    created_at, updated_at
                 FROM ai_jobs WHERE id = ?1",
                params![id.to_string()],
                map_ai_job,
            )
            .optional()?;
        Ok(record)
    }

    pub fn list_ai_jobs(&self, project_id: Uuid) -> Result<Vec<AiJobRecord>, PersistenceError> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, provider_id, provider_name, operation, prompt, status,
                progress, message, result_asset_id, result_path, error_code, user_message,
                created_at, updated_at
             FROM ai_jobs WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 50",
        )?;
        let rows = stmt.query_map(params![project_id.to_string()], map_ai_job)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Update progress only while the job is still running. Returns false when
    /// the job was cancelled/finished in the meantime.
    pub fn update_ai_job_progress(
        &self,
        id: Uuid,
        progress: f32,
        message: Option<&str>,
        updated_at: &str,
    ) -> Result<bool, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "UPDATE ai_jobs SET progress = ?1, message = COALESCE(?2, message), updated_at = ?3
             WHERE id = ?4 AND status = 'running'",
            params![progress as f64, message, updated_at, id.to_string()],
        )?;
        Ok(changed > 0)
    }

    /// Transition queued -> running. Returns false when the job was cancelled
    /// before it started.
    pub fn mark_ai_job_running(
        &self,
        id: Uuid,
        updated_at: &str,
    ) -> Result<bool, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "UPDATE ai_jobs SET status = 'running', progress = 0, updated_at = ?1
             WHERE id = ?2 AND status = 'queued'",
            params![updated_at, id.to_string()],
        )?;
        Ok(changed > 0)
    }

    /// Mark an active job completed. Cancellation wins if it happened while the
    /// provider was finishing.
    pub fn complete_ai_job_if_active(
        &self,
        id: Uuid,
        progress: f32,
        message: Option<&str>,
        result_asset_id: Uuid,
        result_path: &str,
        updated_at: &str,
    ) -> Result<bool, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "UPDATE ai_jobs SET status = 'completed', progress = ?1, message = ?2,
                result_asset_id = ?3, result_path = ?4, error_code = NULL, user_message = NULL,
                updated_at = ?5
             WHERE id = ?6 AND status IN ('queued', 'running')",
            params![
                progress as f64,
                message,
                result_asset_id.to_string(),
                result_path,
                updated_at,
                id.to_string()
            ],
        )?;
        Ok(changed > 0)
    }

    /// Mark an active job failed. Returns false when it was already cancelled or
    /// completed.
    pub fn fail_ai_job_if_active(
        &self,
        id: Uuid,
        error_code: &str,
        user_message: &str,
        updated_at: &str,
    ) -> Result<bool, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "UPDATE ai_jobs SET status = 'failed', error_code = ?1, user_message = ?2,
                updated_at = ?3
             WHERE id = ?4 AND status IN ('queued', 'running')",
            params![error_code, user_message, updated_at, id.to_string()],
        )?;
        Ok(changed > 0)
    }

    pub fn cancel_ai_job_if_active(
        &self,
        id: Uuid,
        user_message: &str,
        updated_at: &str,
    ) -> Result<bool, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "UPDATE ai_jobs SET status = 'cancelled', error_code = 'CANCELLED',
                user_message = ?1, updated_at = ?2
             WHERE id = ?3 AND status IN ('queued', 'running')",
            params![user_message, updated_at, id.to_string()],
        )?;
        Ok(changed > 0)
    }

    pub fn fail_interrupted_jobs(
        &self,
        user_message: &str,
        updated_at: &str,
    ) -> Result<usize, PersistenceError> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "UPDATE ai_jobs SET status = 'failed', error_code = 'INTERRUPTED',
                user_message = ?1, updated_at = ?2
             WHERE status IN ('queued', 'running')",
            params![user_message, updated_at],
        )?;
        Ok(changed)
    }
}

fn get_uuid(row: &Row<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let value: String = row.get(index)?;
    Uuid::parse_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn get_optional_uuid(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Uuid>> {
    let value: Option<String> = row.get(index)?;
    match value {
        Some(value) => Uuid::parse_str(&value).map(Some).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        }),
        None => Ok(None),
    }
}

fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn map_project(row: &Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        id: get_uuid(row, 0)?,
        name: row.get(1)?,
        collection_id: get_uuid(row, 2)?,
        collection_name: row.get(3)?,
        width: row.get::<_, i64>(4)? as u32,
        height: row.get::<_, i64>(5)? as u32,
        document_json: row.get(6)?,
        schema_version: row.get::<_, i64>(7)? as u32,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_asset(row: &Row<'_>) -> rusqlite::Result<AssetRecord> {
    Ok(AssetRecord {
        id: get_uuid(row, 0)?,
        project_id: get_uuid(row, 1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        width: row.get::<_, i64>(4)? as u32,
        height: row.get::<_, i64>(5)? as u32,
        bytes: row.get::<_, i64>(6)? as u64,
        created_at: row.get(7)?,
    })
}

fn map_ai_job(row: &Row<'_>) -> rusqlite::Result<AiJobRecord> {
    Ok(AiJobRecord {
        id: get_uuid(row, 0)?,
        project_id: get_uuid(row, 1)?,
        provider_id: row.get(2)?,
        provider_name: row.get(3)?,
        operation: row.get(4)?,
        prompt: row.get(5)?,
        status: row.get(6)?,
        progress: row.get::<_, Option<f64>>(7)?.map(|v| v as f32),
        message: row.get(8)?,
        result_asset_id: get_optional_uuid(row, 9)?,
        result_path: row.get(10)?,
        error_code: row.get(11)?,
        user_message: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn map_provider(row: &Row<'_>) -> rusqlite::Result<ProviderRecord> {
    Ok(ProviderRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        base_url: row.get(3)?,
        model: row.get(4)?,
        allow_localhost: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_project() -> ProjectRecord {
        let collection_id = Uuid::new_v4();
        ProjectRecord {
            id: Uuid::new_v4(),
            name: "测试项目".into(),
            collection_id,
            collection_name: "测试项目".into(),
            width: 1200,
            height: 800,
            document_json: "{\"id\":\"x\"}".into(),
            schema_version: 1,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    fn sample_asset(project_id: Uuid) -> AssetRecord {
        AssetRecord {
            id: Uuid::new_v4(),
            project_id,
            kind: "source".into(),
            path: PathBuf::from("C:/photos/a.png")
                .to_string_lossy()
                .to_string(),
            width: 1200,
            height: 800,
            bytes: 123456,
            created_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    fn sample_job(project_id: Uuid) -> AiJobRecord {
        AiJobRecord {
            id: Uuid::new_v4(),
            project_id,
            provider_id: "mock".into(),
            provider_name: "本地演示 Provider".into(),
            operation: "edit".into(),
            prompt: "变暖".into(),
            status: "queued".into(),
            progress: Some(0.0),
            message: None,
            result_asset_id: None,
            result_path: None,
            error_code: None,
            user_message: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn project_asset_provider_and_job_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("luma.db");
        let project = sample_project();
        let asset = sample_asset(project.id);
        let job = sample_job(project.id);
        let provider = ProviderRecord {
            id: "mock".into(),
            name: "本地演示 Provider".into(),
            kind: "mock".into(),
            base_url: None,
            model: None,
            allow_localhost: false,
            created_at: "2026-01-01T00:00:00.000Z".into(),
        };
        {
            let db = Database::open(&db_path).unwrap();
            db.save_project(&project).unwrap();
            db.insert_asset(&asset).unwrap();
            db.upsert_provider(&provider).unwrap();
            db.upsert_ai_job(&job).unwrap();
            assert_eq!(db.get_project(project.id).unwrap().unwrap(), project);
            assert_eq!(db.list_project_summaries().unwrap().len(), 1);
            assert_eq!(db.get_asset(asset.id).unwrap().unwrap(), asset);
            assert_eq!(db.get_source_asset(project.id).unwrap().unwrap(), asset);
            assert_eq!(db.get_provider("mock").unwrap().unwrap(), provider);
            assert_eq!(db.get_ai_job(job.id).unwrap().unwrap(), job);
            assert_eq!(db.list_ai_jobs(project.id).unwrap().len(), 1);
        }
        let reopened = Database::open(&db_path).unwrap();
        assert_eq!(reopened.get_project(project.id).unwrap().unwrap(), project);
    }

    #[test]
    fn deleting_an_asset_removes_only_that_row() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("db.sqlite")).unwrap();
        let project = sample_project();
        db.save_project(&project).unwrap();
        let first = sample_asset(project.id);
        let second = AssetRecord {
            id: Uuid::new_v4(),
            ..sample_asset(project.id)
        };
        db.insert_asset(&first).unwrap();
        db.insert_asset(&second).unwrap();

        db.delete_asset(first.id).unwrap();
        assert!(db.get_asset(first.id).unwrap().is_none());
        assert_eq!(db.get_asset(second.id).unwrap().unwrap(), second);
        assert_eq!(db.list_assets(project.id).unwrap().len(), 1);
        // Deleting the same id twice is a no-op, not an error.
        db.delete_asset(first.id).unwrap();
    }

    #[test]
    fn progress_only_updates_running_jobs_and_cancel_is_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("db.sqlite")).unwrap();
        let project = sample_project();
        db.save_project(&project).unwrap();
        let mut job = sample_job(project.id);
        job.status = "running".into();
        db.upsert_ai_job(&job).unwrap();

        assert!(db
            .update_ai_job_progress(job.id, 50.0, Some("处理中"), "2026-01-01T00:00:01.000Z")
            .unwrap());
        assert_eq!(db.get_ai_job(job.id).unwrap().unwrap().progress, Some(50.0));
        assert!(db
            .cancel_ai_job_if_active(job.id, "已取消", "2026-01-01T00:00:02.000Z")
            .unwrap());
        assert_eq!(db.get_ai_job(job.id).unwrap().unwrap().status, "cancelled");
        assert!(!db
            .update_ai_job_progress(job.id, 90.0, None, "2026-01-01T00:00:03.000Z")
            .unwrap());
        assert!(!db
            .cancel_ai_job_if_active(job.id, "已取消", "2026-01-01T00:00:04.000Z")
            .unwrap());
    }

    #[test]
    fn interrupted_jobs_are_failed_on_startup() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("db.sqlite")).unwrap();
        let project = sample_project();
        db.save_project(&project).unwrap();
        let mut job = sample_job(project.id);
        job.status = "running".into();
        db.upsert_ai_job(&job).unwrap();
        let changed = db
            .fail_interrupted_jobs("已中断", "2026-01-01T00:00:05.000Z")
            .unwrap();
        assert_eq!(changed, 1);
        let stored = db.get_ai_job(job.id).unwrap().unwrap();
        assert_eq!(stored.status, "failed");
        assert_eq!(stored.error_code.as_deref(), Some("INTERRUPTED"));
    }

    #[test]
    fn database_is_thread_safe() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Database>();
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn v1_projects_become_single_photo_collections() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("luma.db");
        let id = Uuid::new_v4();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE projects (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    document_json TEXT NOT NULL,
                    schema_version INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO projects VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id.to_string(),
                    "旧项目",
                    640i64,
                    480i64,
                    "{}",
                    12i64,
                    "2026-01-01T00:00:00.000Z",
                    "2026-01-01T00:00:00.000Z"
                ],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1i64).unwrap();
        }

        let db = Database::open(&path).unwrap();
        let project = db.get_project(id).unwrap().unwrap();
        assert_eq!(project.collection_id, id);
        assert_eq!(project.collection_name, "旧项目");
        let summaries = db.list_project_summaries().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].collection_id, id);
        assert_eq!(summaries[0].collection_name, "旧项目");
    }
}
