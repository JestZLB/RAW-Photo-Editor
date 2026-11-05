//! Named snapshots ("versions").
//!
//! Unlike the undo stack these are written to SQLite, so they survive a restart.
//! A snapshot is a full document copy: restoring one never touches the original
//! photo, and the restore itself becomes an undoable history entry.

use ::document::{now_rfc3339, ProjectDocument};
use persistence::VersionRecord;
use uuid::Uuid;

use super::{parse_uuid, AppService, DEFAULT_PREVIEW_MAX, SOURCE_PREVIEW_MAX};
use crate::dto::{DocumentState, VersionSummary};
use crate::error::ApiError;
use crate::requests::{CreateVersionRequest, ProjectIdRequest, VersionIdRequest};

/// A manual snapshot is megabytes each; the oldest are dropped beyond this.
const MAX_VERSIONS_PER_PROJECT: usize = 50;
const MAX_VERSION_NAME_CHARS: usize = 60;

fn version_not_found() -> ApiError {
    ApiError::new(
        "VERSION_NOT_FOUND",
        "version not found",
        "这个版本已经不存在，请刷新列表。",
    )
}

impl AppService {
    pub fn list_versions(
        &self,
        request: ProjectIdRequest,
    ) -> Result<Vec<VersionSummary>, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.version_list(project_id)
    }

    pub fn create_version(
        &self,
        request: CreateVersionRequest,
    ) -> Result<Vec<VersionSummary>, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let name = request.name.trim();
        if name.is_empty() {
            return Err(ApiError::invalid_argument(
                "version name is empty",
                "请先输入版本名称。",
            ));
        }
        if name.chars().count() > MAX_VERSION_NAME_CHARS {
            return Err(ApiError::invalid_argument(
                "version name is too long",
                "版本名称最多 60 个字符。",
            ));
        }

        self.ensure_session(project_id)?;
        let (document, revision) = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| ApiError::internal("session lock is poisoned"))?;
            let project = sessions
                .touch(project_id)
                .ok_or_else(ApiError::project_not_found)?;
            let document = project.session.document();
            (document.clone(), document.revision)
        };

        let thumbnail = self
            .rendered_thumbnail(project_id, revision)
            .or_else(|| self.source_thumbnail(project_id, &document));

        let record = VersionRecord {
            id: Uuid::new_v4(),
            project_id,
            name: name.to_string(),
            document_json: serde_json::to_string(&document)?,
            revision,
            thumbnail_path: thumbnail,
            created_at: now_rfc3339(),
        };
        self.db.insert_version(&record)?;
        self.trim_versions(project_id)?;
        self.version_list(project_id)
    }

    /// Restore a snapshot as the live document. The current state is pushed onto
    /// the undo stack, so a restore can always be undone.
    pub fn restore_version(&self, request: VersionIdRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let version_id = parse_uuid(&request.version_id, "version")?;
        let record = self
            .db
            .get_version(version_id)?
            .ok_or_else(version_not_found)?;
        if record.project_id != project_id {
            return Err(version_not_found());
        }
        let snapshot: ProjectDocument = serde_json::from_str(&record.document_json)?;
        self.ensure_session(project_id)?;
        let label = format!("恢复版本：{}", record.name);
        self.session(project_id, |project| {
            project.session.apply_snapshot(snapshot, &label);
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn delete_version(
        &self,
        request: VersionIdRequest,
    ) -> Result<Vec<VersionSummary>, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let version_id = parse_uuid(&request.version_id, "version")?;
        self.db.delete_version(project_id, version_id)?;
        self.version_list(project_id)
    }

    fn version_list(&self, project_id: Uuid) -> Result<Vec<VersionSummary>, ApiError> {
        Ok(self
            .db
            .list_versions(project_id)?
            .into_iter()
            .map(|row| VersionSummary {
                id: row.id.to_string(),
                name: row.name,
                revision: row.revision,
                created_at: row.created_at,
                // Cached previews are pruned behind our back; drop dead paths
                // instead of handing the UI an image that cannot load.
                thumbnail_path: row
                    .thumbnail_path
                    .filter(|path| std::path::Path::new(path).is_file()),
            })
            .collect())
    }

    fn trim_versions(&self, project_id: Uuid) -> Result<(), ApiError> {
        let rows = self.db.list_versions(project_id)?;
        for row in rows.into_iter().skip(MAX_VERSIONS_PER_PROJECT) {
            self.db.delete_version(project_id, row.id)?;
        }
        Ok(())
    }

    fn rendered_thumbnail(&self, project_id: Uuid, revision: u64) -> Option<String> {
        let path = self.assets.render_preview_path_variant(
            project_id,
            revision,
            DEFAULT_PREVIEW_MAX,
            false,
        );
        path.is_file().then(|| path.to_string_lossy().to_string())
    }

    fn source_thumbnail(&self, project_id: Uuid, document: &ProjectDocument) -> Option<String> {
        let asset = document.source.as_ref()?;
        let path = self
            .assets
            .source_preview_path(project_id, asset.asset_id, SOURCE_PREVIEW_MAX);
        path.is_file().then(|| path.to_string_lossy().to_string())
    }
}
