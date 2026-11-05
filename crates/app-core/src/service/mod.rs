//! Application service shared by the Tauri command layer and integration tests.
//!
//! The service owns the open-document LRU, the SQLite metadata store, the asset
//! cache, provider configuration and background AI jobs. Pixel data never
//! crosses the IPC boundary: commands exchange file paths and JSON only.

mod ai;
mod denoise;
mod document;
mod projects;
mod providers;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ::document::{
    now_rfc3339 as document_now, BlendMode, DocumentSession, LayerKind, ProjectDocument,
    SourceAsset,
};
use ai_provider::{
    now_rfc3339, AiJobInfo, AiJobStatus, ProviderInfo, ProviderKind, ProviderRegistry,
    StoredProviderConfig, MOCK_PROVIDER_ID, MOCK_PROVIDER_NAME,
};
use color_pipeline::Lut;
use image_core::{decode_image_from_path, encode_atomic, DecodedImage, OutputFormat};
use persistence::{
    default_credential_store, AssetStore, CredentialStore, Database, ProjectRecord, ProviderRecord,
};
use task_runtime::TaskRegistry;
use uuid::Uuid;

use crate::dto::{AppInfo, DocumentState, ExportProgress, Histogram, LayerState, SourceAssetState};
use crate::error::ApiError;

pub const MAX_OPEN_PROJECTS: usize = 3;
/// Decoded full-resolution sources are expensive (RGBA8 = 4 bytes/pixel).
/// Keep a bounded total; larger single images are not cached at all and are
/// re-decoded only when a full-resolution operation needs them.
pub const MAX_CACHED_SOURCE_BYTES: usize = 256 * 1024 * 1024;
pub const SOURCE_PREVIEW_MAX: u32 = 1600;
pub const SOURCE_THUMBNAIL_MAX: u32 = 480;
pub const DEFAULT_PREVIEW_MAX: u32 = 1600;
pub const MAX_PREVIEW_DIMENSION: u32 = 8192;
pub const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;
/// Upper bound for a `.cube` LUT file. 64x64x64 float cubes are ~5MB of text.
pub const MAX_LUT_BYTES: usize = 8 * 1024 * 1024;
/// Parsed LUTs kept for the lifetime of the service. A 64³ cube is ~3MB of
/// floats, and re-parsing one on every preview frame dominates the render.
const MAX_CACHED_LUTS: usize = 4;
/// Histograms for already-rendered previews, keyed by cache path. Computing one
/// on a cache hit means decoding the whole PNG again for 4×64 bins.
const MAX_CACHED_HISTOGRAMS: usize = 16;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            cache_dir: cache_dir.into(),
        }
    }
}

/// Adapter used to push `ai-job-updated` events. Implemented by the Tauri layer;
/// tests use [`NoopEventSink`].
pub trait EventSink: Send + Sync {
    fn ai_job_updated(&self, job: &AiJobInfo);

    /// Export progress is optional: callers that do not need a live bar can
    /// leave the default no-op implementation.
    fn export_progress(&self, _progress: &ExportProgress) {}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn ai_job_updated(&self, _job: &AiJobInfo) {}
}

/// Face boxes measured on the analysis proxy. Region measurement reuses them
/// so opening the portrait panel does not run the ONNX detectors twice.
pub(crate) struct CachedDetection {
    pub(crate) faces: Vec<portrait_engine::FaceBox>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) struct OpenProject {
    pub(crate) session: DocumentSession,
    pub(crate) source: Option<Arc<DecodedImage>>,
    pub(crate) last_used: u64,
    /// Cached facial region analysis; detection costs hundreds of ms per frame
    /// and the source pixels never change while the project is open. Regions
    /// are already normalized to the analysis image, so cached results stay
    /// independent of the decoding resolution.
    pub(crate) regions: Vec<crate::dto::PortraitFaceRegions>,
    pub(crate) regions_analysed: bool,
    pub(crate) detection: Option<CachedDetection>,
    /// Decoded 1600px preview proxy, tagged with the asset it came from.
    /// Interactive previews reuse it instead of decoding the PNG again on every
    /// slider tick. A 1600px RGBA8 proxy is ~10 MB and at most
    /// [`MAX_OPEN_PROJECTS`] sessions exist, so the extra footprint is bounded;
    /// it is dropped together with the session or when the asset changes.
    pub(crate) proxy: Option<(Uuid, Arc<DecodedImage>)>,
    /// Set by a command that mutated the document; flushed by `session()` after
    /// the global lock is released.
    pending_save: bool,
    /// Monotonic snapshot counter, so a slow save cannot overwrite a newer one.
    save_seq: u64,
    last_saved_seq: u64,
}

impl OpenProject {
    pub(crate) fn request_save(&mut self) {
        self.pending_save = true;
    }
}

pub(crate) struct SessionManager {
    open: HashMap<Uuid, OpenProject>,
    tick: u64,
}

impl SessionManager {
    fn new() -> Self {
        Self {
            open: HashMap::new(),
            tick: 0,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    fn touch(&mut self, project_id: Uuid) -> Option<&mut OpenProject> {
        let tick = self.next_tick();
        let entry = self.open.get_mut(&project_id)?;
        entry.last_used = tick;
        Some(entry)
    }

    fn contains(&mut self, project_id: Uuid) -> bool {
        self.touch(project_id).is_some()
    }

    fn remove(&mut self, project_id: Uuid) {
        self.open.remove(&project_id);
    }

    /// Drop cached full-resolution pixels from least-recently-used projects
    /// until the total fits the budget. The caller may still hold an `Arc`, so
    /// this only releases the session's extra reference.
    fn trim_source_cache(&mut self, budget: usize) {
        let mut total: usize = self
            .open
            .values()
            .filter_map(|project| project.source.as_ref().map(|source| source.rgba8.len()))
            .sum();
        if total <= budget {
            return;
        }
        let mut candidates: Vec<(Uuid, u64, usize)> = self
            .open
            .iter()
            .filter_map(|(id, project)| {
                project
                    .source
                    .as_ref()
                    .map(|source| (*id, project.last_used, source.rgba8.len()))
            })
            .collect();
        candidates.sort_by_key(|(_, tick, _)| *tick);
        for (id, _, bytes) in candidates {
            if total <= budget {
                break;
            }
            if let Some(project) = self.open.get_mut(&id) {
                project.source = None;
                total = total.saturating_sub(bytes);
            }
        }
    }

    fn insert(
        &mut self,
        project_id: Uuid,
        session: DocumentSession,
        source: Option<Arc<DecodedImage>>,
    ) {
        let tick = self.next_tick();
        self.open.insert(
            project_id,
            OpenProject {
                session,
                source,
                last_used: tick,
                regions: Vec::new(),
                regions_analysed: false,
                detection: None,
                proxy: None,
                pending_save: false,
                save_seq: 0,
                last_saved_seq: 0,
            },
        );
        while self.open.len() > MAX_OPEN_PROJECTS {
            let Some(oldest) = self
                .open
                .iter()
                .min_by_key(|(_, value)| value.last_used)
                .map(|(id, _)| *id)
            else {
                break;
            };
            self.open.remove(&oldest);
        }
    }
}

pub struct AppService {
    paths: AppPaths,
    pub(crate) db: Arc<Database>,
    pub(crate) assets: AssetStore,
    pub(crate) credentials: Arc<dyn CredentialStore>,
    pub(crate) providers: ProviderRegistry,
    pub(crate) events: Arc<dyn EventSink>,
    pub(crate) sessions: Mutex<SessionManager>,
    pub(crate) luts: Mutex<HashMap<Uuid, Lut>>,
    pub(crate) histograms: Mutex<HashMap<PathBuf, Histogram>>,
    pub(crate) denoise_models: Mutex<HashMap<PathBuf, Arc<ai_denoise::DenoiseModel>>>,
    /// Cached Vulkan probe for the bundled Photo-HQ engine.
    pub(crate) photo_hq_probe: Mutex<Option<Result<(), String>>>,
    /// Serialises document writes; kept separate so a slow SQLite save never
    /// holds the session lock.
    save_lock: Mutex<()>,
    pub(crate) tasks: TaskRegistry,
}

impl AppService {
    pub fn new(paths: AppPaths) -> Result<Arc<Self>, ApiError> {
        Self::with_event_sink(paths, Arc::new(NoopEventSink))
    }

    pub fn with_event_sink(
        paths: AppPaths,
        events: Arc<dyn EventSink>,
    ) -> Result<Arc<Self>, ApiError> {
        Self::with_event_sink_and_credentials(paths, events, default_credential_store())
    }

    pub fn with_event_sink_and_credentials(
        paths: AppPaths,
        events: Arc<dyn EventSink>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Result<Arc<Self>, ApiError> {
        std::fs::create_dir_all(&paths.data_dir)?;
        std::fs::create_dir_all(&paths.cache_dir)?;
        let assets = AssetStore::new(paths.data_dir.clone(), paths.cache_dir.clone())?;
        let db = Arc::new(Database::open(&paths.data_dir.join("luma.db"))?);
        db.fail_interrupted_jobs("应用上次退出时任务未完成，请重新提交。", &now_rfc3339())?;
        let providers = load_providers(&db)?;
        Ok(Arc::new(Self {
            paths,
            db,
            assets,
            credentials,
            providers,
            events,
            sessions: Mutex::new(SessionManager::new()),
            luts: Mutex::new(HashMap::new()),
            histograms: Mutex::new(HashMap::new()),
            denoise_models: Mutex::new(HashMap::new()),
            photo_hq_probe: Mutex::new(None),
            save_lock: Mutex::new(()),
            tasks: TaskRegistry::new(),
        }))
    }

    pub fn data_dir(&self) -> &std::path::Path {
        &self.paths.data_dir
    }

    pub fn cache_dir(&self) -> &std::path::Path {
        &self.paths.cache_dir
    }

    pub fn get_app_info(&self) -> AppInfo {
        let raw = raw_develop::capability();
        let mut supported: Vec<String> = image_core::ImageFileFormat::supported_extensions()
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        if raw.available {
            supported.extend(raw.extensions);
        }
        let gpu = render_engine::gpu_available();
        AppInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            backend: if gpu {
                render_engine::BackendKind::Wgpu.as_str()
            } else {
                render_engine::BackendKind::Cpu.as_str()
            }
            .to_string(),
            gpu_available: gpu,
            raw_available: raw.available,
            face_model_available: portrait_engine::face_model_available(),
            workspace_dir: self.paths.data_dir.to_string_lossy().to_string(),
            supported_image_formats: supported,
        }
    }

    // ---- shared helpers -------------------------------------------------

    pub(crate) fn session<R>(
        &self,
        project_id: Uuid,
        f: impl FnOnce(&mut OpenProject) -> Result<R, ApiError>,
    ) -> Result<R, ApiError> {
        self.ensure_session(project_id)?;
        let (outcome, pending) = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| ApiError::internal("session lock is poisoned"))?;
            let project = sessions
                .touch(project_id)
                .ok_or_else(ApiError::project_not_found)?;
            let outcome = f(project);
            let pending = if project.pending_save {
                project.pending_save = false;
                project.save_seq = project.save_seq.saturating_add(1);
                Some((project.save_seq, project.session.document().clone()))
            } else {
                None
            };
            (outcome, pending)
        };
        // The write happens after the lock is released: serialising a mask-heavy
        // document takes tens of milliseconds and must not stall every other
        // project's preview.
        if let Some((seq, document)) = pending {
            if let Err(error) = self.persist_snapshot(project_id, seq, &document) {
                if outcome.is_ok() {
                    return Err(error);
                }
            }
        }
        outcome
    }

    /// Write a document snapshot that was taken under the session lock.
    ///
    /// Writes are serialised separately from session access, and the per-project
    /// sequence number guarantees that a snapshot taken later can never be
    /// overwritten by an older one that lost the race to the disk.
    fn persist_snapshot(
        &self,
        project_id: Uuid,
        seq: u64,
        document: &ProjectDocument,
    ) -> Result<(), ApiError> {
        let _write = self
            .save_lock
            .lock()
            .map_err(|_| ApiError::internal("save lock is poisoned"))?;
        let already_newer = self
            .sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.open.get(&project_id).map(|p| p.last_saved_seq))
            .is_some_and(|saved| saved >= seq);
        if already_newer {
            return Ok(());
        }
        self.save_document(document)?;
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(project) = sessions.open.get_mut(&project_id) {
                project.last_saved_seq = project.last_saved_seq.max(seq);
            }
        }
        Ok(())
    }

    pub(crate) fn ensure_session(&self, project_id: Uuid) -> Result<(), ApiError> {
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| ApiError::internal("session lock is poisoned"))?;
            if sessions.contains(project_id) {
                return Ok(());
            }
        }

        let record = self
            .db
            .get_project(project_id)?
            .ok_or_else(ApiError::project_not_found)?;
        let document: ProjectDocument = serde_json::from_str(&record.document_json)?;
        if document.id != project_id {
            return Err(ApiError::new(
                "INVALID_PROJECT_DATA",
                "project id does not match stored document",
                "项目数据损坏，无法打开。",
            ));
        }
        let mut document = document.normalized();
        self.assets.ensure_project_dirs(project_id)?;
        let repaired_source = self.repair_legacy_raw_orientation(project_id, &mut document)?;

        // Rebuild missing cache previews eagerly so reopening a project shows a
        // thumbnail. A missing source file is not fatal here; render/export
        // surface SOURCE_MISSING with an actionable message.
        let mut source = repaired_source.map(Arc::new);
        if source.is_none() {
            if let Some(source_asset) = &document.source {
                let preview = self.assets.source_preview_path(
                    project_id,
                    source_asset.asset_id,
                    SOURCE_PREVIEW_MAX,
                );
                let thumbnail = self.assets.source_thumbnail_path(
                    project_id,
                    source_asset.asset_id,
                    SOURCE_THUMBNAIL_MAX,
                );
                let original = std::path::Path::new(&source_asset.original_path);
                if (!preview.exists() || !thumbnail.exists()) && original.exists() {
                    let decoded = decode_source_image(original)?;
                    self.write_source_previews(project_id, source_asset.asset_id, &decoded)?;
                    source = Some(Arc::new(decoded));
                }
            }
        }

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ApiError::internal("session lock is poisoned"))?;
        if !sessions.contains(project_id) {
            sessions.insert(project_id, DocumentSession::new(document), source);
        }
        Ok(())
    }

    pub(crate) fn insert_session(
        &self,
        project_id: Uuid,
        session: DocumentSession,
        source: Option<Arc<DecodedImage>>,
    ) -> Result<(), ApiError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ApiError::internal("session lock is poisoned"))?;
        sessions.insert(project_id, session, source);
        Ok(())
    }

    pub(crate) fn save_document(&self, document: &ProjectDocument) -> Result<(), ApiError> {
        let record = ProjectRecord {
            id: document.id,
            name: document.name.clone(),
            collection_id: document.collection_id,
            collection_name: document.collection_name.clone(),
            width: document.canvas.width,
            height: document.canvas.height,
            document_json: serde_json::to_string(document)?,
            schema_version: document.schema_version,
            created_at: document.created_at.clone(),
            updated_at: document.updated_at.clone(),
        };
        self.db.save_project(&record)?;
        Ok(())
    }

    pub(crate) fn document_state(
        &self,
        project_id: Uuid,
        session: &DocumentSession,
    ) -> DocumentState {
        let document = session.document();
        let source = document
            .source
            .as_ref()
            .map(|asset| self.source_state(project_id, asset));
        let layers = document
            .layers
            .iter()
            .map(|layer| {
                let preview_path = match layer.kind {
                    LayerKind::Base => document.source.as_ref().map(|asset| {
                        self.assets.source_preview_path(
                            project_id,
                            asset.asset_id,
                            SOURCE_PREVIEW_MAX,
                        )
                    }),
                    LayerKind::AiResult => layer.asset_id.map(|asset_id| {
                        self.assets
                            .ai_preview_path(project_id, asset_id, SOURCE_PREVIEW_MAX)
                    }),
                    _ => None,
                }
                .filter(|path| path.exists())
                .map(|path| path.to_string_lossy().to_string());
                LayerState {
                    id: layer.id.to_string(),
                    name: layer.name.clone(),
                    kind: layer_kind_label(layer.kind).to_string(),
                    visible: layer.visible,
                    locked: layer.locked,
                    opacity: layer.opacity,
                    blend_mode: blend_label(layer.blend_mode).to_string(),
                    color: layer.color,
                    portrait: layer.portrait,
                    retouch: layer.retouch.clone(),
                    mask: layer.mask.clone(),
                    // Only AI result layers expose an asset id in the contract.
                    asset_id: if layer.kind == LayerKind::AiResult {
                        layer.asset_id.map(|id| id.to_string())
                    } else {
                        None
                    },
                    preview_path,
                    created_at: layer.created_at.clone(),
                }
            })
            .collect();
        DocumentState {
            id: document.id.to_string(),
            name: document.name.clone(),
            collection_id: document.collection_id.to_string(),
            collection_name: document.collection_name.clone(),
            schema_version: document.schema_version,
            revision: document.revision,
            created_at: document.created_at.clone(),
            updated_at: document.updated_at.clone(),
            canvas: crate::dto::CanvasState {
                width: document.canvas.width,
                height: document.canvas.height,
                working_color_space: document.canvas.working_color_space.clone(),
            },
            crop: crate::dto::CropState {
                x: document.crop.x,
                y: document.crop.y,
                width: document.crop.width,
                height: document.crop.height,
                angle: document.crop.angle,
            },
            source,
            layers,
            can_undo: session.can_undo(),
            can_redo: session.can_redo(),
            history: session.history(),
        }
    }

    fn source_state(&self, project_id: Uuid, source: &SourceAsset) -> SourceAssetState {
        SourceAssetState {
            asset_id: source.asset_id.to_string(),
            display_name: source.display_name.clone(),
            width: source.width,
            height: source.height,
            format: source.format.clone(),
            original_path: source.original_path.clone(),
            preview_path: self
                .assets
                .source_preview_path(project_id, source.asset_id, SOURCE_PREVIEW_MAX)
                .to_string_lossy()
                .to_string(),
            thumbnail_path: self
                .assets
                .source_thumbnail_path(project_id, source.asset_id, SOURCE_THUMBNAIL_MAX)
                .to_string_lossy()
                .to_string(),
            color_space: source.color_space.clone(),
            has_source_profile: source.icc_asset_id.is_some(),
        }
    }

    pub(crate) fn write_source_previews(
        &self,
        project_id: Uuid,
        asset_id: Uuid,
        decoded: &DecodedImage,
    ) -> Result<(), ApiError> {
        self.assets.ensure_project_dirs(project_id)?;
        let preview = decoded.resize_to_fit(SOURCE_PREVIEW_MAX)?;
        encode_atomic(
            &self
                .assets
                .source_preview_path(project_id, asset_id, SOURCE_PREVIEW_MAX),
            &preview,
            OutputFormat::Png,
            100,
        )?;
        let thumbnail = decoded.resize_to_fit(SOURCE_THUMBNAIL_MAX)?;
        encode_atomic(
            &self
                .assets
                .source_thumbnail_path(project_id, asset_id, SOURCE_THUMBNAIL_MAX),
            &thumbnail,
            OutputFormat::Png,
            100,
        )?;
        // The proxy on disk was just rewritten from new pixels, so any decoded
        // copy kept for this session is stale.
        self.drop_source_proxy(project_id);
        Ok(())
    }

    /// Decoded 1600px proxy for interactive previews, when already available.
    /// Returns `None` for a different asset so a replaced source can never be
    /// rendered from a stale decode.
    fn cached_source_proxy(&self, project_id: Uuid, asset_id: Uuid) -> Option<Arc<DecodedImage>> {
        let sessions = self.sessions.lock().ok()?;
        let project = sessions.open.get(&project_id)?;
        match project.proxy.as_ref() {
            Some((cached_asset, proxy)) if *cached_asset == asset_id => Some(proxy.clone()),
            _ => None,
        }
    }

    fn store_source_proxy(&self, project_id: Uuid, asset_id: Uuid, proxy: Arc<DecodedImage>) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        if let Some(project) = sessions.touch(project_id) {
            project.proxy = Some((asset_id, proxy));
        }
    }

    fn drop_source_proxy(&self, project_id: Uuid) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        if let Some(project) = sessions.open.get_mut(&project_id) {
            project.proxy = None;
        }
    }

    pub(crate) fn load_source_image(
        &self,
        project_id: Uuid,
        document: &ProjectDocument,
    ) -> Result<Arc<DecodedImage>, ApiError> {
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| ApiError::internal("session lock is poisoned"))?;
            if let Some(project) = sessions.touch(project_id) {
                if let Some(source) = &project.source {
                    return Ok(source.clone());
                }
            }
        }

        let source = document.source.as_ref().ok_or_else(|| {
            ApiError::new(
                "SOURCE_MISSING",
                "project has no source image",
                "项目尚未导入照片，无法渲染。",
            )
        })?;
        let path = std::path::Path::new(&source.original_path);
        if !path.exists() {
            return Err(ApiError::source_missing());
        }
        let decoded = Arc::new(decode_source_image(path)?);
        let bytes = decoded.rgba8.len();
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| ApiError::internal("session lock is poisoned"))?;
        // A single 100MP photo is already 400MB; caching it would defeat the
        // budget, so only reasonably sized sources are retained.
        if bytes <= MAX_CACHED_SOURCE_BYTES / 2 {
            if let Some(project) = sessions.touch(project_id) {
                project.source = Some(decoded.clone());
            }
        }
        sessions.trim_source_cache(MAX_CACHED_SOURCE_BYTES);
        Ok(decoded)
    }

    /// Load the small analysis proxy used by tone/white-balance/face commands.
    /// These operations only need representative pixels; decoding a 100MP RAW
    /// for a 256-bin histogram or a 640px face detector wastes gigabytes.
    pub(crate) fn load_analysis_image(
        &self,
        project_id: Uuid,
        document: &ProjectDocument,
    ) -> Result<Arc<DecodedImage>, ApiError> {
        let source = document.source.as_ref().ok_or_else(|| {
            ApiError::new(
                "SOURCE_MISSING",
                "project has no source image",
                "项目尚未导入照片，无法分析。",
            )
        })?;
        let preview_path =
            self.assets
                .source_preview_path(project_id, source.asset_id, SOURCE_PREVIEW_MAX);
        if preview_path.is_file() {
            if let Ok(image) = decode_source_image(&preview_path) {
                return Ok(Arc::new(image));
            }
        }
        let full = self.load_source_image(project_id, document)?;
        let resized = full.resize_to_fit(SOURCE_PREVIEW_MAX)?;
        Ok(Arc::new(resized))
    }

    pub(crate) fn provider_record(&self, provider_id: &str) -> Result<ProviderRecord, ApiError> {
        self.db.get_provider(provider_id)?.ok_or_else(|| {
            ApiError::new(
                "PROVIDER_NOT_FOUND",
                "provider not found",
                "Provider 不存在或已被删除。",
            )
        })
    }

    pub(crate) fn provider_info(&self, record: &ProviderRecord) -> Result<ProviderInfo, ApiError> {
        let kind = ProviderKind::from_label(&record.kind)
            .ok_or_else(|| ApiError::internal("unknown provider kind in database"))?;
        let stored = StoredProviderConfig {
            id: record.id.clone(),
            name: record.name.clone(),
            kind,
            base_url: record.base_url.clone(),
            model: record.model.clone(),
            allow_localhost: record.allow_localhost,
            created_at: record.created_at.clone(),
        };
        Ok(ProviderRegistry::info_for(
            &stored,
            self.credentials.has(&record.id),
        ))
    }
}

fn load_providers(db: &Database) -> Result<ProviderRegistry, ApiError> {
    let mut records = db.list_providers()?;
    if !records.iter().any(|record| record.id == MOCK_PROVIDER_ID) {
        let mock = ProviderRecord {
            id: MOCK_PROVIDER_ID.to_string(),
            name: MOCK_PROVIDER_NAME.to_string(),
            kind: ProviderKind::Mock.as_str().to_string(),
            base_url: None,
            model: None,
            allow_localhost: false,
            created_at: document_now(),
        };
        db.upsert_provider(&mock)?;
        records.push(mock);
    }
    let registry = ProviderRegistry::new();
    for record in records {
        let Some(kind) = ProviderKind::from_label(&record.kind) else {
            continue;
        };
        registry.upsert(StoredProviderConfig {
            id: record.id,
            name: record.name,
            kind,
            base_url: record.base_url,
            model: record.model,
            allow_localhost: record.allow_localhost,
            created_at: record.created_at,
        });
    }
    Ok(registry)
}

pub(crate) fn decode_source_image(path: &std::path::Path) -> Result<DecodedImage, ApiError> {
    if raw_develop::is_raw_path(path) {
        raw_develop::decode_raw(path).map_err(ApiError::from)
    } else {
        decode_image_from_path(path).map_err(ApiError::from)
    }
}

pub(crate) fn parse_uuid(value: &str, what: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value.trim()).map_err(|_| {
        ApiError::invalid_argument(
            format!("invalid {what} id: {value}"),
            "标识无效，请重新打开项目或刷新列表。",
        )
    })
}

pub(crate) fn layer_kind_label(kind: LayerKind) -> &'static str {
    match kind {
        LayerKind::Base => "base",
        LayerKind::Portrait => "portrait",
        LayerKind::Color => "color",
        LayerKind::AiResult => "aiResult",
        LayerKind::Retouch => "retouch",
    }
}

pub(crate) fn blend_label(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::SoftLight => "softLight",
    }
}

pub(crate) fn job_info(record: &persistence::AiJobRecord) -> AiJobInfo {
    AiJobInfo {
        id: record.id.to_string(),
        project_id: record.project_id.to_string(),
        provider_id: record.provider_id.clone(),
        provider_name: record.provider_name.clone(),
        operation: ai_provider::AiJobOperation::from_label(&record.operation),
        prompt: record.prompt.clone(),
        status: AiJobStatus::from_label(&record.status),
        progress: record.progress,
        message: record.message.clone(),
        result_asset_id: record.result_asset_id.map(|id| id.to_string()),
        result_path: record.result_path.clone(),
        error_code: record.error_code.clone(),
        user_message: record.user_message.clone(),
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use ::document::{DocumentSession, ProjectDocument};
    use image_core::DecodedImage;

    fn tiny_source() -> Arc<DecodedImage> {
        Arc::new(DecodedImage::new(5, 5, vec![0u8; 5 * 5 * 4]).unwrap())
    }

    #[test]
    fn full_source_cache_is_trimmed_from_least_recently_used() {
        let mut sessions = SessionManager::new();
        let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        for (index, id) in ids.iter().enumerate() {
            sessions.insert(
                *id,
                DocumentSession::new(ProjectDocument::new("test", 5, 5)),
                None,
            );
            let project = sessions.open.get_mut(id).unwrap();
            project.last_used = index as u64;
            project.source = Some(tiny_source());
        }
        // Three 100-byte sources; a 250-byte budget drops exactly the oldest.
        sessions.trim_source_cache(250);
        assert!(sessions.open[&ids[0]].source.is_none());
        assert!(sessions.open[&ids[1]].source.is_some());
        assert!(sessions.open[&ids[2]].source.is_some());
    }
}
