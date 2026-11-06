//! Named snapshots ("versions").
//!
//! Unlike the undo stack these are written to SQLite, so they survive a restart.
//! A snapshot is a full document copy: restoring one never touches the original
//! photo, and the restore itself becomes an undoable history entry.

use std::time::Instant;

use ::document::{now_rfc3339, ProjectDocument};
use persistence::VersionRecord;
use uuid::Uuid;

use super::{
    parse_uuid, AppService, DEFAULT_PREVIEW_MAX, MAX_PREVIEW_DIMENSION, SOURCE_PREVIEW_MAX,
};
use crate::dto::{DocumentState, PreviewResult, VersionSummary};
use crate::error::ApiError;
use crate::requests::{
    CreateVersionRequest, ProjectIdRequest, RenderVersionPreviewRequest, VersionIdRequest,
};

/// A manual snapshot is a full document copy; the oldest are dropped beyond
/// this many. Bytes are capped separately because a mask-heavy document is
/// megabytes — 50 of them would otherwise consume hundreds of MB.
const MAX_VERSIONS_PER_PROJECT: usize = 50;
/// Refuse a single pathological snapshot instead of admitting unbounded rows.
const MAX_VERSION_BYTES_PER_SNAPSHOT: usize = 4 * 1024 * 1024;
/// Total JSON budget per project (preview caches are separate and pruned).
const MAX_VERSION_BYTES_PER_PROJECT: u64 = 32 * 1024 * 1024;
const MAX_VERSION_NAME_CHARS: usize = 60;

/// Newest-first `(id, bytes)` → ids to delete. Both limits apply; once the byte
/// budget is exceeded every older snapshot goes, which keeps the result
/// deterministic. The newest snapshot always survives — a user must never see
/// the version they just saved disappear because of older ones.
fn versions_to_drop(sizes: &[(Uuid, u64)], max_count: usize, max_bytes: u64) -> Vec<Uuid> {
    let mut total = 0u64;
    let mut drop = Vec::new();
    for (index, (id, bytes)) in sizes.iter().enumerate() {
        total = total.saturating_add(*bytes);
        if index > 0 && (index >= max_count || total > max_bytes) {
            drop.push(*id);
        }
    }
    drop
}

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

        let document_json = serde_json::to_string(&document)?;
        if document_json.len() > MAX_VERSION_BYTES_PER_SNAPSHOT {
            return Err(ApiError::invalid_argument(
                "snapshot exceeds the per-version size limit",
                "当前文档过大，无法保存为版本；请先精简蒙版或修复点后重试。",
            ));
        }
        let record = VersionRecord {
            id: Uuid::new_v4(),
            project_id,
            name: name.to_string(),
            document_json,
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
        self.assets.delete_version_previews(project_id, version_id);
        self.version_list(project_id)
    }

    /// Render a snapshot so the UI can compare it with the current edit.
    ///
    /// The snapshot is immutable, so the cache file is valid for as long as the
    /// version row exists; it uses a `version-` prefix and is therefore not
    /// touched by the live-preview rotation.
    pub fn render_version_preview(
        &self,
        request: RenderVersionPreviewRequest,
    ) -> Result<PreviewResult, ApiError> {
        let started = Instant::now();
        let project_id = parse_uuid(&request.project_id, "project")?;
        let version_id = parse_uuid(&request.version_id, "version")?;
        let max_dimension = request.max_dimension.unwrap_or(DEFAULT_PREVIEW_MAX);
        if max_dimension == 0 || max_dimension > MAX_PREVIEW_DIMENSION {
            return Err(ApiError::invalid_argument(
                format!("invalid preview max dimension: {max_dimension}"),
                "预览尺寸无效。",
            ));
        }
        let record = self
            .db
            .get_version(version_id)?
            .ok_or_else(version_not_found)?;
        if record.project_id != project_id {
            return Err(version_not_found());
        }
        let document: ProjectDocument = serde_json::from_str(&record.document_json)?;
        let cache_path = self
            .assets
            .version_preview_path(project_id, version_id, max_dimension);
        self.render_preview_into(project_id, &document, max_dimension, &cache_path, started)
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
        let sizes = self.db.version_sizes(project_id)?;
        for id in versions_to_drop(
            &sizes,
            MAX_VERSIONS_PER_PROJECT,
            MAX_VERSION_BYTES_PER_PROJECT,
        ) {
            self.db.delete_version(project_id, id)?;
            // Automatic eviction must not leave orphaned preview PNGs behind —
            // manual deletion already does this.
            self.assets.delete_version_previews(project_id, id);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(values: &[u64]) -> Vec<(Uuid, u64)> {
        values
            .iter()
            .enumerate()
            .map(|(index, bytes)| (Uuid::from_u128(index as u128 + 1), *bytes))
            .collect()
    }

    #[test]
    fn drops_oldest_beyond_the_count_limit() {
        let rows = sizes(&[10, 10, 10, 10]);
        assert_eq!(versions_to_drop(&rows, 2, 1000), vec![rows[2].0, rows[3].0]);
    }

    #[test]
    fn drops_everything_past_the_byte_budget() {
        // The second row already crosses 100 bytes; every older one goes too.
        let rows = sizes(&[60, 60, 1]);
        assert_eq!(versions_to_drop(&rows, 50, 100), vec![rows[1].0, rows[2].0]);
    }

    #[test]
    fn keeps_all_when_within_both_limits() {
        let rows = sizes(&[30, 30, 30]);
        assert!(versions_to_drop(&rows, 50, 100).is_empty());
    }

    #[test]
    fn never_drops_the_newest_snapshot() {
        // A lowered limit or a legacy oversized row must not make the version
        // the user just saved disappear.
        let rows = sizes(&[200]);
        assert!(versions_to_drop(&rows, 50, 100).is_empty());
    }

    #[test]
    fn empty_history_needs_no_trim() {
        assert!(versions_to_drop(&[], 50, 100).is_empty());
    }
}
