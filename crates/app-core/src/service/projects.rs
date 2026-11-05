use std::path::{Path, PathBuf};

use ai_provider::AiJobStatus;
use base64::Engine;
use document::{
    now_rfc3339, CropRect, DocumentSession, Layer, LayerKind, ProjectDocument, SourceAsset,
};
use image_core::{
    decode_source_from_memory, decode_source_from_path, detect_image_format,
    detect_image_format_from_memory,
};
use persistence::{sanitize_file_name, AssetRecord};
use uuid::Uuid;

use super::{parse_uuid, AppService, MAX_IMPORT_BYTES, MAX_LUT_BYTES, SOURCE_THUMBNAIL_MAX};
use crate::dto::{
    CacheClearResult, CollectionSummary, DocumentState, LutAssetResult, ProjectSummary,
    SyncColorAdjustmentsResult,
};
use crate::error::ApiError;
use crate::requests::{
    CreateProjectRequest, ImportLutRequest, ImportPhotoBytesRequest, ImportPhotoRequest,
    OpenProjectRequest, ProjectIdRequest, RenameCollectionRequest, RenameProjectRequest,
    SyncColorAdjustmentsRequest,
};

impl AppService {
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, ApiError> {
        let records = self.db.list_project_summaries()?;
        let mut summaries = Vec::with_capacity(records.len());
        for record in records {
            summaries.push(self.summary_from_record(&record)?);
        }
        Ok(summaries)
    }

    /// One entry per collection, newest first. Photos with zero dimensions are
    /// empty sets created without a first photo: they keep the collection alive
    /// but do not count as photos, and the menu still opens the newest real
    /// photo when one exists.
    pub fn list_collections(&self) -> Result<Vec<CollectionSummary>, ApiError> {
        let mut collections: Vec<CollectionSummary> = Vec::new();
        let mut latest_photos: Vec<Option<String>> = Vec::new();
        let mut indexes: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for project in self.list_projects()? {
            let has_photo = project.width > 0 && project.height > 0;
            if let Some(&index) = indexes.get(&project.collection_id) {
                if has_photo {
                    collections[index].photo_count += 1;
                    if latest_photos[index].is_none() {
                        latest_photos[index] = Some(project.id);
                        collections[index].cover_thumbnail_path = project.thumbnail_path;
                    }
                }
                continue;
            }
            indexes.insert(project.collection_id.clone(), collections.len());
            latest_photos.push(if has_photo { Some(project.id.clone()) } else { None });
            collections.push(CollectionSummary {
                id: project.collection_id,
                name: project.collection_name,
                photo_count: usize::from(has_photo),
                updated_at: project.updated_at,
                cover_thumbnail_path: if has_photo { project.thumbnail_path } else { None },
                latest_project_id: project.id,
            });
        }
        for (index, photo) in latest_photos.into_iter().enumerate() {
            if let Some(photo) = photo {
                collections[index].latest_project_id = photo;
            }
        }
        Ok(collections)
    }

    /// Repair RAW projects imported before orientation was applied at decode
    /// time. Only edits that are independent of pixel coordinates are allowed;
    /// otherwise the stored crop/masks would no longer match the rotated image.
    pub(crate) fn repair_legacy_raw_orientation(
        &self,
        project_id: Uuid,
        document: &mut ProjectDocument,
    ) -> Result<Option<image_core::DecodedImage>, ApiError> {
        let Some(source_asset) = document.source.as_ref() else {
            return Ok(None);
        };
        if document.canvas.width <= document.canvas.height
            || !document
                .crop
                .is_full(document.canvas.width, document.canvas.height)
            || document.layers.iter().any(|layer| {
                layer.mask.is_some()
                    || matches!(layer.kind, LayerKind::AiResult | LayerKind::Retouch)
            })
        {
            return Ok(None);
        }
        let path = Path::new(&source_asset.original_path);
        if !raw_develop::is_raw_path(path) {
            return Ok(None);
        }
        let Some(orientation) = raw_develop::raw_orientation(path) else {
            return Ok(None);
        };
        if !(5..=8).contains(&orientation) {
            return Ok(None);
        }
        let decoded = match raw_develop::decode_raw(path) {
            Ok(decoded) => decoded,
            Err(_) => return Ok(None),
        };
        // Orientation 5..=8 swaps the axes. If the stored canvas already has
        // the decoded size this project was imported correctly.
        if decoded.width != document.canvas.height || decoded.height != document.canvas.width {
            return Ok(None);
        }
        let asset_id = source_asset.asset_id;
        if self
            .write_source_previews(project_id, asset_id, &decoded)
            .is_err()
        {
            return Ok(None);
        }

        let mut repaired = document.clone();
        repaired.canvas.width = decoded.width;
        repaired.canvas.height = decoded.height;
        repaired.crop = CropRect::full(decoded.width, decoded.height);
        if let Some(source) = repaired.source.as_mut() {
            source.width = decoded.width;
            source.height = decoded.height;
        }
        repaired.revision = repaired.revision.saturating_add(1);
        repaired.updated_at = now_rfc3339();
        self.save_document(&repaired)?;
        if let Some(mut asset) = self.db.get_source_asset(project_id)? {
            asset.width = decoded.width;
            asset.height = decoded.height;
            self.db.insert_asset(&asset)?;
        }
        *document = repaired;
        Ok(Some(decoded))
    }

    fn summary_from_record(
        &self,
        record: &persistence::ProjectSummaryRecord,
    ) -> Result<ProjectSummary, ApiError> {
        let thumbnail_path = match self.db.get_source_asset(record.id)? {
            Some(asset) => {
                let path =
                    self.assets
                        .source_thumbnail_path(record.id, asset.id, SOURCE_THUMBNAIL_MAX);
                path.exists().then(|| path.to_string_lossy().to_string())
            }
            None => None,
        };
        Ok(ProjectSummary {
            id: record.id.to_string(),
            name: record.name.clone(),
            collection_id: record.collection_id.to_string(),
            collection_name: record.collection_name.clone(),
            width: record.width,
            height: record.height,
            updated_at: record.updated_at.clone(),
            thumbnail_path,
        })
    }

    pub fn rename_project(
        &self,
        request: RenameProjectRequest,
    ) -> Result<ProjectSummary, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let name = normalize_project_name(&request.name)?;
        self.ensure_session(project_id)?;
        self.session(project_id, |project| {
            project.session.rename(name.clone());
            project.request_save();
            Ok(())
        })?;
        let record = self
            .db
            .get_project(project_id)?
            .ok_or_else(ApiError::project_not_found)?;
        self.summary_from_record(&persistence::ProjectSummaryRecord {
            id: record.id,
            name: record.name,
            collection_id: record.collection_id,
            collection_name: record.collection_name,
            width: record.width,
            height: record.height,
            updated_at: record.updated_at,
        })
    }

    /// Rename a photo set. Every project in the collection stores its own copy of
/// `collectionName`, so all of them are rewritten; open sessions are updated in
/// memory as well. Metadata only: no revision bump, no undo entry.
pub fn rename_collection(&self, request: RenameCollectionRequest) -> Result<(), ApiError> {
    let collection_id = parse_uuid(&request.collection_id, "collection")?;
    let name = normalize_project_name(&request.name)?;
    let summaries = self.db.list_project_summaries()?;
    let members: Vec<Uuid> = summaries
        .iter()
        .filter(|summary| summary.collection_id == collection_id)
        .map(|summary| summary.id)
        .collect();
    if members.is_empty() {
        return Err(ApiError::invalid_argument(
            "collection does not exist",
            "这个照片集不存在或已被删除。",
        ));
    }
    for project_id in &members {
        let record = self
            .db
            .get_project(*project_id)?
            .ok_or_else(ApiError::project_not_found)?;
        let mut document: ProjectDocument = serde_json::from_str(&record.document_json)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        document.collection_name = name.clone();
        let document_json =
            serde_json::to_string(&document).map_err(|error| ApiError::internal(error.to_string()))?;
        self.db.save_project(&persistence::ProjectRecord {
            collection_name: name.clone(),
            document_json,
            ..record
        })?;
    }
    let mut sessions = self
        .sessions
        .lock()
        .map_err(|_| ApiError::internal("session lock is poisoned"))?;
    for project_id in &members {
        if let Some(project) = sessions.touch(*project_id) {
            project.session.rename_collection(name.clone());
        }
    }
    Ok(())
}

/// Replace the colour stack of every listed photo with the source photo's
/// colour stack.
///
/// Only colour layers travel: crop, portrait and retouch layers depend on the
/// source composition and measured faces, so copying them would silently
/// misplace masks and warps on a different photo. Masks inside colour layers are
/// normalized, so they remain valid. LUT files are project-scoped, so each
/// target gets its own managed copy; an unreadable LUT is dropped instead of
/// leaving a dangling reference.
pub fn sync_color_adjustments(
    &self,
    request: SyncColorAdjustmentsRequest,
) -> Result<SyncColorAdjustmentsResult, ApiError> {
    let source_id = parse_uuid(&request.source_project_id, "project")?;
    let source = self
        .db
        .get_project(source_id)?
        .ok_or_else(ApiError::project_not_found)?;
    let (templates, lut_payloads) = self.session(source_id, |project| {
        let mut templates: Vec<Layer> = Vec::new();
        let mut luts: std::collections::HashMap<Uuid, Vec<u8>> = std::collections::HashMap::new();
        for layer in project.session.document().layers.iter() {
            if layer.kind != LayerKind::Color || layer.color.is_none() {
                continue;
            }
            templates.push(layer.clone());
            let lut_id = layer
                .color
                .filter(document::ColorParams::uses_lut)
                .and_then(|color| color.lut_asset_id)
                .filter(|id| !luts.contains_key(id));
            if let Some(lut_id) = lut_id {
                let record = self.db.get_asset(lut_id).ok().flatten();
                if let Some(record) = record {
                    if let Ok(bytes) = std::fs::read(&record.path) {
                        luts.insert(lut_id, bytes);
                    }
                }
            }
        }
        Ok((templates, luts))
    })?;
    if templates.is_empty() {
        return Err(ApiError::invalid_argument(
            "source project has no colour adjustment",
            "当前照片还没有调色调整，先调好一张再同步。",
        ));
    }

    let mut seen = std::collections::HashSet::new();
    let mut synced = 0usize;
    let mut failed = Vec::new();
    for raw in &request.project_ids {
        let Ok(target_id) = parse_uuid(raw, "project") else {
            failed.push(raw.clone());
            continue;
        };
        if target_id == source_id || !seen.insert(target_id) {
            continue;
        }
        let target = self.db.get_project(target_id)?;
        let Some(target) = target else {
            failed.push(target_id.to_string());
            continue;
        };
        if target.collection_id != source.collection_id {
            failed.push(target.name);
            continue;
        }
        match self.apply_color_templates(target_id, &templates, &lut_payloads) {
            Ok(()) => synced += 1,
            Err(_) => failed.push(target.name),
        }
    }
    Ok(SyncColorAdjustmentsResult { synced, failed })
}

fn apply_color_templates(
    &self,
    target_id: Uuid,
    templates: &[Layer],
    lut_payloads: &std::collections::HashMap<Uuid, Vec<u8>>,
) -> Result<(), ApiError> {
    // Copy LUT files before touching the document so a failed copy leaves the
    // target untouched; assets created here are removed again if the document
    // mutation fails.
    let mut created_assets = Vec::new();
    let mut lut_ids = std::collections::HashMap::new();
    let outcome = (|| -> Result<(), ApiError> {
        for template in templates {
            let source_lut = template
                .color
                .and_then(|color| color.lut_asset_id)
                .filter(|id| lut_payloads.contains_key(id));
            let Some(source_lut) = source_lut else {
                lut_ids.insert(template.id, None);
                continue;
            };
            let bytes = &lut_payloads[&source_lut];
            let asset_id = Uuid::new_v4();
            self.assets.ensure_project_dirs(target_id)?;
            let path = self.assets.lut_path(target_id, asset_id);
            self.assets.write_atomic(&path, bytes)?;
            self.db.insert_asset(&AssetRecord {
                id: asset_id,
                project_id: target_id,
                kind: "lut".to_string(),
                path: path.to_string_lossy().to_string(),
                width: 0,
                height: 0,
                bytes: bytes.len() as u64,
                created_at: now_rfc3339(),
            })?;
            created_assets.push(asset_id);
            lut_ids.insert(template.id, Some(asset_id));
        }
        self.session(target_id, |project| {
            project.session.try_mutate(|document| {
                let insert_at = document
                    .layers
                    .iter()
                    .position(|layer| layer.kind == LayerKind::Color)
                    .unwrap_or_else(|| document.layers.len().min(1));
                document.layers.retain(|layer| layer.kind != LayerKind::Color);
                let insert_at = insert_at.min(document.layers.len());
                for (offset, template) in templates.iter().enumerate() {
                    let mut layer = template.clone();
                    layer.id = Uuid::new_v4();
                    layer.created_at = now_rfc3339();
                    layer.locked = false;
                    if let Some(color) = layer.color.as_mut() {
                        color.lut_asset_id = lut_ids.get(&template.id).copied().flatten();
                    }
                    document.layers.insert(insert_at + offset, layer);
                }
                Ok::<(), ApiError>(())
            })?;
            project.session.commit("同步调色调整");
            project.request_save();
            Ok(())
        })
    })();
    if outcome.is_err() {
        for asset_id in created_assets {
            let _ = std::fs::remove_file(self.assets.lut_path(target_id, asset_id));
            let _ = self.db.delete_asset(asset_id);
        }
    }
    outcome
}

pub fn delete_project(&self, request: ProjectIdRequest) -> Result<(), ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        if self.db.get_project(project_id)?.is_none() {
            return Err(ApiError::project_not_found());
        }
        let has_active_job = self.db.list_ai_jobs(project_id)?.iter().any(|job| {
            matches!(
                AiJobStatus::from_label(&job.status),
                AiJobStatus::Queued | AiJobStatus::Running
            )
        });
        if has_active_job {
            return Err(ApiError::invalid_argument(
                "project has an active AI job",
                "这个项目还有正在进行的 AI 任务，请先取消任务再删除。",
            ));
        }
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| ApiError::internal("session lock is poisoned"))?;
            sessions.remove(project_id);
        }
        self.db.delete_project(project_id)?;
        self.assets.remove_project_files(project_id);
        Ok(())
    }

    pub fn clear_render_cache(&self) -> Result<CacheClearResult, ApiError> {
        let (deleted_files, freed_bytes) = self.assets.clear_render_cache()?;
        Ok(CacheClearResult {
            deleted_files,
            freed_bytes,
        })
    }

    pub fn create_project(&self, request: CreateProjectRequest) -> Result<DocumentState, ApiError> {
        let name = normalize_project_name(&request.name)?;
        let document = ProjectDocument::empty_project(name);
        self.save_document(&document)?;
        let session = DocumentSession::new(document);
        let project_id = session.document().id;
        let state = self.document_state(project_id, &session);
        self.insert_session(project_id, session, None)?;
        Ok(state)
    }

    pub fn open_project(&self, request: OpenProjectRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        self.ensure_session(project_id)?;
        self.session(project_id, |project| {
            Ok(self.document_state(project_id, &project.session))
        })
    }

    pub fn import_photo(&self, request: ImportPhotoRequest) -> Result<DocumentState, ApiError> {
        let raw_path = request.path.trim();
        if raw_path.is_empty() {
            return Err(ApiError::invalid_argument(
                "import path is empty",
                "请选择要导入的照片。",
            ));
        }
        let path = PathBuf::from(raw_path);
        let absolute = absolute_path(&path);
        if !absolute.exists() {
            return Err(ApiError::new(
                "IO",
                format!("file does not exist: {}", absolute.display()),
                "找不到所选照片文件，请确认路径正确。",
            ));
        }
        let file_bytes = std::fs::metadata(&absolute)
            .map_err(|e| image_core::ImageError::Io(e.to_string()))?
            .len();
        let (format_label, decoded, color) = if raw_develop::is_raw_path(&absolute) {
            let extension = raw_develop::raw_extension(&absolute).unwrap_or_else(|| "raw".into());
            // RAW developers emit an sRGB rendering; the camera profile is not
            // applied here, so nothing is remapped.
            (
                extension,
                raw_develop::decode_raw(&absolute)?,
                ImportedColor::default(),
            )
        } else {
            let format = detect_image_format(&absolute)?;
            // Decoding validates file size, dimensions and content before any
            // project/asset row is written.
            let source = decode_source_from_path(&absolute)?;
            let color = ImportedColor::from_source(&source);
            (format.label().to_string(), source.image, color)
        };

        let project_id = Uuid::new_v4();
        let asset_id = Uuid::new_v4();
        let fallback_extension = if format_label == "jpeg" {
            "jpg"
        } else {
            format_label.as_str()
        };
        let display_name = display_name_for(&absolute, fallback_extension);
        let project_name = project_name_from(&absolute, fallback_extension);
        let (collection_id, collection_name) = self.resolve_import_collection(
            request.collection_id.as_deref(),
            request.collection_name.as_deref(),
            &project_name,
        )?;
        self.persist_imported_source(ImportedSource {
            project_id,
            asset_id,
            display_name,
            project_name,
            collection_id,
            collection_name,
            original_path: absolute.to_string_lossy().to_string(),
            format_label,
            decoded,
            file_bytes,
            color,
        })
    }

    pub fn import_photo_bytes(
        &self,
        request: ImportPhotoBytesRequest,
    ) -> Result<DocumentState, ApiError> {
        let mut encoded = request.data_base64.trim();
        if let Some((prefix, rest)) = encoded.split_once(',') {
            if prefix.starts_with("data:") && prefix.contains("base64") {
                encoded = rest.trim();
            }
        }
        if encoded.is_empty() {
            return Err(ApiError::invalid_argument(
                "empty base64 payload",
                "示例照片数据为空。",
            ));
        }
        let max_encoded = MAX_IMPORT_BYTES + MAX_IMPORT_BYTES / 3 + 16;
        if encoded.len() > max_encoded {
            return Err(ApiError::new(
                "IMAGE_TOO_LARGE",
                "base64 payload exceeds import limit",
                "示例照片过大（最大 32MB）。",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded))
            .map_err(|e| {
                ApiError::invalid_argument(
                    format!("invalid base64: {e}"),
                    "示例照片数据无效，无法导入。",
                )
            })?;
        if bytes.len() > MAX_IMPORT_BYTES {
            return Err(ApiError::new(
                "IMAGE_TOO_LARGE",
                "decoded import exceeds 32MB",
                "示例照片过大（最大 32MB）。",
            ));
        }
        let format = detect_image_format_from_memory(&bytes)?;
        let source = decode_source_from_memory(&bytes)?;
        let color = ImportedColor::from_source(&source);
        let decoded = source.image;

        let project_id = Uuid::new_v4();
        let asset_id = Uuid::new_v4();
        let display_name = sanitize_file_name(&request.file_name);
        let display_name = if display_name == "unnamed" {
            format!("import.{}", format.extension())
        } else {
            display_name
        };
        let project_name = project_name_from(Path::new(&display_name), format.extension());
        let (collection_id, collection_name) = self.resolve_import_collection(
            request.collection_id.as_deref(),
            request.collection_name.as_deref(),
            &project_name,
        )?;
        self.assets.ensure_project_dirs(project_id)?;
        let managed_path =
            self.assets
                .imported_source_path(project_id, asset_id, format.extension());
        self.assets.write_atomic(&managed_path, &bytes)?;
        self.persist_imported_source(ImportedSource {
            project_id,
            asset_id,
            display_name,
            project_name,
            collection_id,
            collection_name,
            original_path: managed_path.to_string_lossy().to_string(),
            format_label: format.label().to_string(),
            decoded,
            file_bytes: bytes.len() as u64,
            color,
        })
    }

    /// Copies a `.cube` LUT into managed project storage and registers it as an
    /// asset. The document is untouched: the UI commits `lutAssetId` through the
    /// normal parameter path so the change stays undoable.
    pub fn import_lut(&self, request: ImportLutRequest) -> Result<LutAssetResult, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        if self.db.get_project(project_id)?.is_none() {
            return Err(ApiError::project_not_found());
        }
        let io_error =
            |message: String| ApiError::new("IO", message, "无法读取 LUT 文件，请重试。");
        let too_large = || {
            ApiError::new(
                "LUT_TOO_LARGE",
                "cube file exceeds 8MB",
                "LUT 文件过大（最大 8MB）。",
            )
        };
        let (bytes, raw_name) = match (
            request
                .path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            request.data_base64.as_deref(),
        ) {
            (Some(path), _) => {
                let absolute = absolute_path(Path::new(path));
                if !absolute.exists() {
                    return Err(ApiError::new(
                        "IO",
                        format!("lut file does not exist: {}", absolute.display()),
                        "找不到所选 LUT 文件，请确认路径正确。",
                    ));
                }
                let len = std::fs::metadata(&absolute)
                    .map_err(|error| io_error(error.to_string()))?
                    .len();
                if len as usize > MAX_LUT_BYTES {
                    return Err(too_large());
                }
                let name = absolute
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_default();
                (
                    std::fs::read(&absolute).map_err(|error| io_error(error.to_string()))?,
                    name,
                )
            }
            (None, Some(encoded)) => {
                let bytes = decode_cube_base64(encoded)?;
                if bytes.len() > MAX_LUT_BYTES {
                    return Err(too_large());
                }
                let name = request
                    .file_name
                    .clone()
                    .unwrap_or_else(|| "lookup.cube".to_string());
                (bytes, name)
            }
            _ => {
                return Err(ApiError::invalid_argument(
                    "lut source is empty",
                    "请选择一个 .cube LUT 文件。",
                ))
            }
        };
        let display_name = sanitize_file_name(&raw_name);
        if !display_name.to_ascii_lowercase().ends_with(".cube") {
            return Err(ApiError::new(
                "LUT_INVALID",
                "selected file is not a .cube LUT",
                "请选择 .cube 格式的 LUT 文件。",
            ));
        }
        let lut = color_pipeline::parse_cube(&bytes).map_err(|message| {
            ApiError::new(
                "LUT_INVALID",
                message,
                "这个 .cube 文件无法解析，请确认它是有效的 1D/3D LUT。",
            )
        })?;

        let asset_id = Uuid::new_v4();
        self.assets.ensure_project_dirs(project_id)?;
        let managed_path = self.assets.lut_path(project_id, asset_id);
        self.assets.write_atomic(&managed_path, &bytes)?;
        self.db.insert_asset(&AssetRecord {
            id: asset_id,
            project_id,
            kind: "lut".to_string(),
            path: managed_path.to_string_lossy().to_string(),
            width: 0,
            height: 0,
            bytes: bytes.len() as u64,
            created_at: now_rfc3339(),
        })?;
        Ok(LutAssetResult {
            asset_id: asset_id.to_string(),
            display_name,
            dimension: lut.dimension_label().to_string(),
            size: lut.size(),
            file_bytes: bytes.len() as u64,
        })
    }

    fn resolve_import_collection(
        &self,
        collection_id: Option<&str>,
        collection_name: Option<&str>,
        fallback_name: &str,
    ) -> Result<(Uuid, String), ApiError> {
        if let Some(raw) = collection_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let id = parse_uuid(raw, "collection")?;
            let name = self.db.collection_name_for(id)?.ok_or_else(|| {
                ApiError::invalid_argument(
                    "collection does not exist",
                    "这个项目不存在或已被删除，请重新选择。",
                )
            })?;
            return Ok((id, name));
        }
        let name = collection_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_name);
        let name = normalize_project_name(name)?;
        Ok((Uuid::new_v4(), name))
    }

    fn persist_imported_source(&self, import: ImportedSource) -> Result<DocumentState, ApiError> {
        let ImportedSource {
            project_id,
            asset_id,
            display_name,
            project_name,
            collection_id,
            collection_name,
            original_path,
            format_label,
            decoded,
            file_bytes,
            color,
        } = import;
        self.assets.ensure_project_dirs(project_id)?;
        self.write_source_previews(project_id, asset_id, &decoded)?;

        let mut document = ProjectDocument::new(project_name, decoded.width, decoded.height);
        document.id = project_id;
        document.collection_id = collection_id;
        document.collection_name = collection_name;
        document.source = Some(SourceAsset {
            asset_id,
            display_name: display_name.clone(),
            width: decoded.width,
            height: decoded.height,
            format: format_label,
            original_path: original_path.clone(),
            file_bytes,
            color_space: color.gamut.clone(),
            icc_asset_id: None,
        });
        // The base layer is part of the initial document construction; general
        // layer insertion (which rejects base layers) is not used here.
        document
            .layers
            .push(Layer::base("原始照片", Some(asset_id)));
        document.updated_at = now_rfc3339();

        // A remapped source keeps its profile so exports can go back to it. The
        // file and the document reference are prepared up front; the asset row
        // has to wait for the project row to exist.
        let profile_asset = color
            .profile
            .as_ref()
            .map(|bytes| (Uuid::new_v4(), bytes.clone()));
        if let Some((profile_id, _)) = &profile_asset {
            if let Some(source) = document.source.as_mut() {
                source.icc_asset_id = Some(*profile_id);
            }
        }

        // Project first (assets have a foreign key to it), then the source row.
        self.save_document(&document)?;
        self.db.insert_asset(&AssetRecord {
            id: asset_id,
            project_id,
            kind: "source".to_string(),
            path: original_path,
            width: decoded.width,
            height: decoded.height,
            bytes: file_bytes,
            created_at: now_rfc3339(),
        })?;
        if let Some((profile_id, bytes)) = &profile_asset {
            let profile_path = self.assets.icc_path(project_id, *profile_id);
            self.assets.write_atomic(&profile_path, bytes)?;
            self.db.insert_asset(&AssetRecord {
                id: *profile_id,
                project_id,
                kind: "icc".to_string(),
                path: profile_path.to_string_lossy().to_string(),
                width: 0,
                height: 0,
                bytes: bytes.len() as u64,
                created_at: now_rfc3339(),
            })?;
        }

        let session = DocumentSession::new(document);
        let state = self.document_state(project_id, &session);
        self.insert_session(project_id, session, Some(std::sync::Arc::new(decoded)))?;
        Ok(state)
    }
}

/// Everything needed to turn a freshly decoded file into a saved project.
struct ImportedSource {
    project_id: Uuid,
    asset_id: Uuid,
    display_name: String,
    project_name: String,
    collection_id: Uuid,
    collection_name: String,
    original_path: String,
    format_label: String,
    decoded: image_core::DecodedImage,
    file_bytes: u64,
    color: ImportedColor,
}

/// Colour metadata learned while importing a photo.
#[derive(Debug, Clone, Default)]
pub(crate) struct ImportedColor {
    /// Human-readable gamut label, e.g. `sRGB` or `Display P3`.
    pub gamut: String,
    /// Profile bytes, present only when pixels were remapped into the working
    /// space and an export may need to map them back.
    pub profile: Option<Vec<u8>>,
}

impl ImportedColor {
    fn from_source(source: &image_core::DecodedSource) -> Self {
        Self {
            gamut: source.source_gamut.label().to_string(),
            profile: source.profile.as_ref().map(|profile| profile.bytes.clone()),
        }
    }
}

fn normalize_project_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::invalid_argument(
            "project name is empty",
            "请输入项目名称。",
        ));
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    Ok(if cleaned.trim().is_empty() {
        "未命名项目".to_string()
    } else {
        cleaned
    })
}

fn project_name_from(path: &Path, fallback_extension: &str) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未命名项目");
    let cleaned: String = stem.chars().filter(|c| !c.is_control()).take(80).collect();
    if cleaned.trim().is_empty() {
        format!("未命名项目.{fallback_extension}")
    } else {
        cleaned
    }
}

fn display_name_for(path: &Path, fallback_extension: &str) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(sanitize_file_name)
        .unwrap_or_else(|| format!("photo.{fallback_extension}"));
    if name == "unnamed" {
        format!("photo.{fallback_extension}")
    } else {
        name
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Accepts either a raw base64 string or a `data:...;base64,` URL.
fn decode_cube_base64(encoded: &str) -> Result<Vec<u8>, ApiError> {
    let invalid =
        |detail: String| ApiError::invalid_argument(detail, "LUT 文件内容无效，无法导入。");
    let mut payload = encoded.trim();
    if let Some((prefix, rest)) = payload.split_once(',') {
        if prefix.starts_with("data:") && prefix.contains("base64") {
            payload = rest.trim();
        }
    }
    if payload.is_empty() {
        return Err(invalid("empty base64 payload".into()));
    }
    if payload.len() > MAX_LUT_BYTES + MAX_LUT_BYTES / 3 + 16 {
        return Err(ApiError::new(
            "LUT_TOO_LARGE",
            "cube payload exceeds 8MB",
            "LUT 文件过大（最大 8MB）。",
        ));
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(payload))
        .map_err(|error| invalid(format!("invalid base64: {error}")))
}
