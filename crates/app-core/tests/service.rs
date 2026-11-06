use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use app_core::{
    AddLayerKind, AddLayerRequest, AiCapabilityRequest, AiDenoiseRequest, AiJobInfo,
    AiJobOperation, AiJobStatus, ApiError, AppPaths, AppService, ApplyAiResultRequest,
    CancelExportRequest, CommitChangesRequest, CreateProjectRequest, CreateVersionRequest,
    DeleteLayerRequest, DuplicateLayerRequest, EventSink, ExportBitDepth, ExportColorSpace,
    ExportImageFormat, ExportImageRequest, ExportMetadata, ExportOptions, ExportProgress,
    ImportLutRequest, ImportPhotoRequest, ListAiJobsRequest, NoopEventSink, OpenProjectRequest,
    PhotoHqDenoiseRequest, PickWhiteBalanceRequest, ProjectIdRequest, ProviderConfigInput,
    ProviderKind, RenameCollectionRequest, RenameProjectRequest, RenderPreviewRequest,
    RenderVersionPreviewRequest, SetCropRequest, SetLayerMaskRequest, SetProviderCredentialRequest,
    SubmitAiJobRequest, SyncColorAdjustmentsRequest, UpdateLayerParamsRequest, UpdateLayerRequest,
    VersionIdRequest, WatermarkOptions, WatermarkPosition,
};
use document::{
    ColorParams, CropRect, LayerMask, LayerPatch, MaskGradient, MaskGradientKind, MaskPoint,
    MaskRegion, MaskStroke, PortraitParams, RetouchMode, RetouchParams, RetouchSpot,
};
use image_core::{encode_atomic, DecodedImage, OutputFormat};
use persistence::{CredentialStore, MemoryCredentialStore};
use uuid::Uuid;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<AiJobInfo>>,
    exports: Mutex<Vec<ExportProgress>>,
}

impl EventSink for RecordingSink {
    fn ai_job_updated(&self, job: &AiJobInfo) {
        self.events.lock().unwrap().push(job.clone());
    }

    fn export_progress(&self, progress: &ExportProgress) {
        self.exports.lock().unwrap().push(progress.clone());
    }
}

impl RecordingSink {
    fn snapshot(&self) -> Vec<AiJobInfo> {
        self.events.lock().unwrap().clone()
    }

    fn export_snapshot(&self) -> Vec<ExportProgress> {
        self.exports.lock().unwrap().clone()
    }
}

struct TestApp {
    service: Arc<AppService>,
    credentials: Arc<MemoryCredentialStore>,
    events: Arc<RecordingSink>,
    dir: tempfile::TempDir,
}

impl TestApp {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let credentials = Arc::new(MemoryCredentialStore::new());
        let events = Arc::new(RecordingSink::default());
        let service = AppService::with_event_sink_and_credentials(
            AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
            events.clone(),
            credentials.clone(),
        )
        .unwrap();
        Self {
            service,
            credentials,
            events,
            dir,
        }
    }

    fn sample_path(&self) -> std::path::PathBuf {
        let path = self.dir.path().join("sample.png");
        write_sample(&path);
        path
    }
}

fn write_sample(path: &Path) {
    let width = 64u32;
    let height = 48u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&[(x * 3) as u8, (y * 4) as u8, ((x + y) * 2) as u8, 255]);
        }
    }
    let image = DecodedImage::new(width, height, rgba).unwrap();
    encode_atomic(path, &image, OutputFormat::Png, 100).unwrap();
}

fn import(app: &TestApp) -> app_core::DocumentState {
    let path = app.sample_path();
    app.service
        .import_photo(ImportPhotoRequest {
            path: path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: None,
        })
        .unwrap()
}

#[test]
fn import_creates_project_and_cached_previews_without_touching_original() {
    let app = TestApp::new();
    let sample = app.sample_path();
    let original_bytes = fs::read(&sample).unwrap();
    let state = import(&app);

    assert_eq!(state.canvas.width, 64);
    assert_eq!(state.canvas.height, 48);
    assert_eq!(state.layers.len(), 1);
    assert_eq!(state.layers[0].kind, "base");
    // Only aiResult layers expose assetId per the contract.
    assert!(state.layers[0].asset_id.is_none());
    let source = state.source.expect("source state");
    assert_eq!(source.format, "png");
    assert!(Path::new(&source.preview_path).exists());
    assert!(Path::new(&source.thumbnail_path).exists());
    assert_eq!(source.original_path, sample.to_string_lossy().to_string());

    // Original is recorded, never copied or modified by path imports.
    assert_eq!(fs::read(&sample).unwrap(), original_bytes);
    assert!(!state.layers.is_empty());

    let projects = app.service.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert!(projects[0]
        .thumbnail_path
        .as_ref()
        .is_some_and(|p| Path::new(p).exists()));

    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: state.id.clone(),
        })
        .unwrap();
    assert_eq!(reopened.id, state.id);
    assert_eq!(reopened.source.as_ref().unwrap().width, 64);
}

#[test]
fn render_preview_uses_revision_cache_and_proxy_dimensions() {
    let app = TestApp::new();
    let state = import(&app);
    let first = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(!first.cached);
    assert_eq!((first.width, first.height), (32, 24));
    assert_eq!(first.revision, state.revision);
    assert!(Path::new(&first.path).exists());

    let second = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(second.cached);
    assert_eq!(second.revision, first.revision);
    assert_eq!(second.path, first.path);

    let first_histogram = first.histogram.expect("a fresh render returns a histogram");
    let second_histogram = second
        .histogram
        .expect("cache hits still expose the histogram, from the cache");
    assert_eq!(first_histogram.red, second_histogram.red);
    assert_eq!(first_histogram.green, second_histogram.green);
    assert_eq!(first_histogram.blue, second_histogram.blue);
    assert_eq!(first_histogram.luma, second_histogram.luma);
    assert!(
        first_histogram.luma.iter().sum::<u32>() > 0,
        "the histogram must describe actual pixels"
    );
}

/// The drag path must not echo the layer payload: masks and retouch points are
/// the heavy part of a document, and the caller only merges revision/history
/// from this response.
#[test]
fn meta_update_returns_the_envelope_without_layers() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();

    let meta = app
        .service
        .update_layer_params_meta(UpdateLayerParamsRequest {
            project_id: state.id.clone(),
            layer_id,
            color: Some(ColorParams {
                exposure: 0.5,
                ..Default::default()
            }),
            portrait: None,
            retouch: None,
        })
        .unwrap();

    assert!(
        meta.layers.is_empty(),
        "meta responses must not carry the layer payload"
    );
    assert_eq!(meta.revision, state.revision + 1);

    // The write itself still landed: reopening shows the new exposure.
    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: state.id.clone(),
        })
        .unwrap();
    assert_eq!(reopened.layers.last().unwrap().color.unwrap().exposure, 0.5);
}

/// Versions are the only history that survives a restart, and restoring one must
/// stay undoable — otherwise a mistaken restore would destroy work.
#[test]
fn named_versions_survive_a_restart_and_restore_as_undoable_steps() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let project_id = state.id.clone();
    let layer_id = state.layers.last().unwrap().id.clone();

    let mut params = ColorParams {
        exposure: 1.0,
        ..Default::default()
    };
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            color: Some(params),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "曝光 1".into(),
        })
        .unwrap();

    let versions = app
        .service
        .create_version(CreateVersionRequest {
            project_id: project_id.clone(),
            name: "暖调".into(),
        })
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].name, "暖调");
    let version_id = versions[0].id.clone();

    params.exposure = 2.0;
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            color: Some(params),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "曝光 2".into(),
        })
        .unwrap();

    let restored = app
        .service
        .restore_version(VersionIdRequest {
            project_id: project_id.clone(),
            version_id: version_id.clone(),
        })
        .unwrap();
    let exposure = |state: &app_core::DocumentState| {
        state
            .layers
            .iter()
            .find(|layer| layer.id == layer_id)
            .and_then(|layer| layer.color)
            .map(|color| color.exposure)
            .unwrap()
    };
    assert_eq!(exposure(&restored), 1.0);
    assert_eq!(restored.history.last().unwrap(), "恢复版本：暖调");

    let undone = app
        .service
        .undo(ProjectIdRequest {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert_eq!(exposure(&undone), 2.0);

    // A second service over the same directory is an application restart.
    let restarted = AppService::with_event_sink(
        AppPaths::new(app.dir.path().join("data"), app.dir.path().join("cache")),
        Arc::new(NoopEventSink),
    )
    .unwrap();
    let listed = restarted
        .list_versions(ProjectIdRequest {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "暖调");
    assert_eq!(listed[0].id, version_id);
}

#[test]
fn version_preview_renders_once_then_hits_its_own_cache() {
    let app = TestApp::new();
    let state = import(&app);
    let project_id = state.id.clone();
    let versions = app
        .service
        .create_version(CreateVersionRequest {
            project_id: project_id.clone(),
            name: "定稿".into(),
        })
        .unwrap();
    let version_id = versions[0].id.clone();

    let first = app
        .service
        .render_version_preview(RenderVersionPreviewRequest {
            project_id: project_id.clone(),
            version_id: version_id.clone(),
            max_dimension: Some(320),
        })
        .unwrap();
    assert!(!first.cached);
    assert!(std::path::Path::new(&first.path).is_file());
    assert!(first.width <= 320 && first.height <= 320);
    assert!(
        first.path.contains(&format!("version-{version_id}-d320")),
        "version previews must not share the live render key: {}",
        first.path
    );

    let second = app
        .service
        .render_version_preview(RenderVersionPreviewRequest {
            project_id: project_id.clone(),
            version_id: version_id.clone(),
            max_dimension: Some(320),
        })
        .unwrap();
    assert!(second.cached);
    assert_eq!(second.path, first.path);

    // Deleting the snapshot also drops its cached renders.
    app.service
        .delete_version(VersionIdRequest {
            project_id,
            version_id,
        })
        .unwrap();
    assert!(!std::path::Path::new(&first.path).exists());
}

/// A raster mask must confine an adjustment to the selected pixels: this is the
/// seam a segmentation model will write into.
#[test]
fn raster_mask_confines_an_adjustment_to_the_selected_half() {
    use render_engine::RasterCoverage;

    fn half_means(path: &str) -> (f32, f32) {
        let image = image_core::decode_image_from_path(std::path::Path::new(path)).unwrap();
        let width = image.width as usize;
        let half = width / 2;
        let mut left = 0f64;
        let mut right = 0f64;
        for y in 0..image.height as usize {
            for x in 0..width {
                let at = (y * width + x) * 4;
                let luma = 0.2126 * image.rgba8[at] as f64
                    + 0.7152 * image.rgba8[at + 1] as f64
                    + 0.0722 * image.rgba8[at + 2] as f64;
                if x < half {
                    left += luma;
                } else {
                    right += luma;
                }
            }
        }
        let count = (image.height as usize * half) as f64;
        ((left / count) as f32, (right / count) as f32)
    }

    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let project_id = state.id.clone();
    let layer_id = state.layers.last().unwrap().id.clone();

    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            color: Some(ColorParams {
                exposure: 1.0,
                ..Default::default()
            }),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "曝光".into(),
        })
        .unwrap();

    let baseline = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(320),
            ignore_crop: false,
        })
        .unwrap();
    let (baseline_left, baseline_right) = half_means(&baseline.path);

    // Left half selected, right half not.
    let coverage = RasterCoverage::from_values(2, 1, vec![1.0, 0.0]).unwrap();
    let masked = app
        .service
        .apply_raster_mask(
            Uuid::parse_str(&project_id).unwrap(),
            Uuid::parse_str(&layer_id).unwrap(),
            &coverage,
            "test",
        )
        .unwrap();
    let layer = masked
        .layers
        .iter()
        .find(|layer| layer.id == layer_id)
        .unwrap();
    let raster = layer
        .mask
        .as_ref()
        .and_then(|mask| mask.raster.as_ref())
        .unwrap();
    assert_eq!(raster.source, "test");
    assert_ne!(raster.asset_id, uuid::Uuid::nil());

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(320),
            ignore_crop: false,
        })
        .unwrap();
    let (masked_left, masked_right) = half_means(&preview.path);
    // `baseline` already has the adjustment everywhere; the mask must keep it on
    // the selected half and take it away from the rest. The 2-pixel coverage is
    // deliberately tiny, so bilinear bleed at the seam is expected.
    assert!(
        (masked_left - baseline_left).abs() < 4.0,
        "selected half must keep the adjustment: {masked_left} vs {baseline_left}"
    );
    assert!(
        baseline_right - masked_right > 20.0,
        "unselected half must lose the adjustment: {masked_right} vs {baseline_right}"
    );

    // A later brush stroke must not silently drop the raster coverage.
    let brushed = app
        .service
        .set_layer_mask(SetLayerMaskRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            mask: Some(LayerMask {
                strokes: vec![MaskStroke {
                    points: vec![MaskPoint { x: 0.25, y: 0.25 }],
                    radius: 0.1,
                    feather: 0.2,
                    erase: false,
                }],
                ..LayerMask::default()
            }),
        })
        .unwrap();
    let kept = brushed
        .layers
        .iter()
        .find(|layer| layer.id == layer_id)
        .and_then(|layer| layer.mask.as_ref())
        .and_then(|mask| mask.raster.as_ref())
        .expect("brush edits keep the raster mask");
    assert_eq!(kept.asset_id, raster.asset_id);
}

#[test]
fn slider_params_commit_once_and_undo_redo_are_monotonic() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let color_layer = state.layers.last().unwrap();
    assert_eq!(color_layer.kind, "color");
    assert_eq!(state.history, vec!["添加调色图层".to_string()]);
    let layer_id = color_layer.id.clone();
    let project_id = state.id.clone();
    let revision_before = state.revision;

    let mut params = ColorParams {
        exposure: 1.0,
        ..Default::default()
    };
    let state = app
        .service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            color: Some(params),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    assert_eq!(state.revision, revision_before + 1);
    assert_eq!(state.layers.last().unwrap().color.unwrap().exposure, 1.0);
    // Slider movement did not create history until commit.
    assert_eq!(state.history.len(), 1);

    params.exposure = 1.5;
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            color: Some(params),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    let committed = app
        .service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "曝光".into(),
        })
        .unwrap();
    assert_eq!(
        committed.history,
        vec!["添加调色图层".to_string(), "曝光".to_string()]
    );
    assert_eq!(
        committed.layers.last().unwrap().color.unwrap().exposure,
        1.5
    );

    let undo_revision = committed.revision;
    let undone = app
        .service
        .undo(app_core::ProjectIdRequest {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert!(undone.revision > undo_revision);
    assert_eq!(undone.layers.last().unwrap().color.unwrap().exposure, 0.0);
    assert!(undone.can_redo);

    let redo_revision = undone.revision;
    let redone = app
        .service
        .redo(app_core::ProjectIdRequest {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert!(redone.revision > redo_revision);
    assert_eq!(redone.layers.last().unwrap().color.unwrap().exposure, 1.5);

    let base_id = redone.layers[0].id.clone();
    let error = app
        .service
        .delete_layer(DeleteLayerRequest {
            project_id,
            layer_id: base_id,
        })
        .unwrap_err();
    assert_eq!(error.code, "BASE_LAYER_PROTECTED");
}

#[test]
fn opacity_drag_creates_one_history_entry_and_labels_visibility() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let project_id = state.id.clone();

    for opacity in [0.9, 0.7, 0.5] {
        let next = app
            .service
            .update_layer(UpdateLayerRequest {
                project_id: project_id.clone(),
                layer_id: layer_id.clone(),
                patch: LayerPatch {
                    opacity: Some(opacity),
                    ..Default::default()
                },
            })
            .unwrap();
        assert_eq!(
            next.history,
            vec!["添加调色图层".to_string()],
            "live opacity updates must not create history"
        );
    }

    let committed = app
        .service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "调整图层不透明度".into(),
        })
        .unwrap();
    assert_eq!(
        committed.history,
        vec!["添加调色图层".to_string(), "调整图层不透明度".to_string()]
    );
    assert!((committed.layers.last().unwrap().opacity - 0.5).abs() < 1e-6);

    let hidden = app
        .service
        .update_layer(UpdateLayerRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            patch: LayerPatch {
                visible: Some(false),
                ..Default::default()
            },
        })
        .unwrap();
    assert_eq!(hidden.history.last().map(String::as_str), Some("隐藏图层"));
    assert!(!hidden.layers.last().unwrap().visible);

    let undone = app
        .service
        .undo(app_core::ProjectIdRequest { project_id })
        .unwrap();
    assert!(undone.layers.last().unwrap().visible);
    assert!((undone.layers.last().unwrap().opacity - 0.5).abs() < 1e-6);
}

#[test]
fn face_portrait_layer_takes_params_and_mask_in_one_step() {
    let app = TestApp::new();
    let state = import(&app);
    let history_before = state.history.len();
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Portrait,
            name: Some("人脸 2 美颜".into()),
            portrait: Some(PortraitParams {
                smooth: 22.0,
                face_index: Some(1),
                ..Default::default()
            }),
            mask: Some(LayerMask {
                regions: vec![MaskRegion {
                    kind: "skin".into(),
                    center_x: 0.62,
                    center_y: 0.45,
                    radius_x: 0.08,
                    radius_y: 0.12,
                    angle: 0.0,
                    feather: 0.55,
                    strength: 1.0,
                }],
                ..Default::default()
            }),
        })
        .unwrap();
    let layer = state.layers.last().unwrap();
    assert_eq!(layer.name, "人脸 2 美颜");
    assert_eq!(layer.portrait.as_ref().unwrap().face_index, Some(1));
    assert_eq!(layer.mask.as_ref().unwrap().regions.len(), 1);
    assert_eq!(state.history.len(), history_before + 1);
}

#[test]
fn portrait_params_are_stored_and_sanitized() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Portrait,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let portrait = PortraitParams {
        smooth: 500.0,
        texture: f32::NAN,
        ..Default::default()
    };
    let state = app
        .service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: state.id.clone(),
            layer_id,
            color: None,
            portrait: Some(portrait),
            retouch: None,
        })
        .unwrap();
    let stored = state.layers.last().unwrap().portrait.unwrap();
    assert_eq!(stored.smooth, 100.0);
    assert_eq!(stored.texture, 70.0);
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id,
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());
}

#[test]
fn crop_is_non_destructive_and_drives_preview_and_export() {
    let app = TestApp::new();
    let state = import(&app);
    let project_id = state.id.clone();

    let cropped = app
        .service
        .set_crop(SetCropRequest {
            project_id: project_id.clone(),
            crop: CropRect {
                x: 16,
                y: 8,
                width: 32,
                height: 24,
                angle: 0.0,
            },
        })
        .unwrap();
    assert_eq!(
        (
            cropped.crop.x,
            cropped.crop.y,
            cropped.crop.width,
            cropped.crop.height
        ),
        (16, 8, 32, 24)
    );
    assert_eq!(cropped.history.last().map(String::as_str), Some("裁剪照片"));
    // The original source metadata is untouched.
    assert_eq!(cropped.source.as_ref().unwrap().width, 64);

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert_eq!((preview.width, preview.height), (32, 24));

    let out = app.dir.path().join("cropped.png");
    let exported = app
        .service
        .export_image(ExportImageRequest {
            project_id: project_id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!((exported.width, exported.height), (32, 24));

    let square = app.dir.path().join("square.png");
    let squared = app
        .service
        .export_image(ExportImageRequest {
            project_id: project_id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: square.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: None,
                width: Some(48),
                height: Some(48),

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!((squared.width, squared.height), (48, 48));

    let undone = app
        .service
        .undo(app_core::ProjectIdRequest { project_id })
        .unwrap();
    assert!(undone.crop.width == 64 && undone.crop.height == 48);
}

#[test]
#[ignore = "requires a real RAW sample; set LUMA_RAW_SAMPLE"]
fn raw_sample_imports_previews_and_exports() {
    let path = std::env::var("LUMA_RAW_SAMPLE").expect("set LUMA_RAW_SAMPLE");
    let app = TestApp::new();
    let state = app
        .service
        .import_photo(ImportPhotoRequest {
            path,
            collection_id: None,
            collection_name: None,
        })
        .unwrap();
    let source = state.source.as_ref().expect("RAW source");
    assert!(!source.preview_path.is_empty());
    assert!(Path::new(&source.preview_path).exists());
    assert!(state.canvas.width > 0 && state.canvas.height > 0);

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(512),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());
    assert!(preview.width <= 512 && preview.height <= 512);

    let out = app.dir.path().join("raw-export.png");
    let exported = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id,
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: Some(512),
                width: None,
                height: None,

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert!(out.exists());
    assert!(exported.width <= 512 && exported.height <= 512);
}

#[test]
fn embedded_face_model_is_reported_and_available() {
    let app = TestApp::new();
    let state = import(&app);
    let info = app.service.get_app_info();
    assert!(info.face_model_available);
    assert!(info.raw_available);
    assert!(info
        .supported_image_formats
        .iter()
        .any(|item| item == "dng"));
    let analysis = app
        .service
        .analyze_portrait(app_core::ProjectIdRequest {
            project_id: state.id,
        })
        .unwrap();
    assert!(analysis.available);
    assert!(analysis.faces.is_empty());
    assert!(analysis.warnings.is_empty());
}

#[test]
fn duplicate_layer_command_commits_history_and_undoes() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let duplicated = app
        .service
        .duplicate_layer(DuplicateLayerRequest {
            project_id: state.id.clone(),
            layer_id,
        })
        .unwrap();
    assert_eq!(duplicated.layers.len(), 3);
    assert!(duplicated.layers.last().unwrap().name.ends_with("副本"));
    assert!(duplicated
        .history
        .last()
        .is_some_and(|label| label == "复制图层"));

    let undone = app
        .service
        .undo(app_core::ProjectIdRequest {
            project_id: state.id,
        })
        .unwrap();
    assert_eq!(undone.layers.len(), 2);
}

#[test]
fn export_emits_progress_and_cancel_unknown_job_is_a_noop() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("progress.png");
    let job_id = "export-progress-test";
    let result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: Some(job_id.to_string()),
            options: ExportOptions {
                watermark: None,
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 90,
                max_dimension: None,
                width: None,
                height: None,

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert!(out.exists());
    assert!(result.bytes > 0);

    let events = app.events.export_snapshot();
    assert!(events.iter().all(|event| event.job_id == job_id));
    assert!(events.iter().any(|event| event.phase == "preparing"));
    assert!(events.iter().any(|event| event.phase == "rendering"));
    assert!(events.iter().any(|event| event.phase == "encoding"));
    assert!(events
        .last()
        .is_some_and(|event| event.phase == "completed" && event.percent == Some(100)));
    assert!(!app.service.cancel_export(CancelExportRequest {
        job_id: "missing-export".into(),
    }));
}

#[test]
fn auto_tone_and_preview_histogram_are_available() {
    let app = TestApp::new();
    let state = import(&app);
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(48),
            ignore_crop: false,
        })
        .unwrap();
    let histogram = preview.histogram.expect("preview histogram");
    assert_eq!(histogram.red.len(), 64);
    assert_eq!(histogram.luma.len(), 64);
    assert!(histogram.luma.iter().sum::<u32>() > 0);

    let suggestion = app
        .service
        .auto_tone(app_core::ProjectIdRequest {
            project_id: state.id,
        })
        .unwrap();
    assert!((-1.0..=1.0).contains(&suggestion.exposure));
    assert!((-10.0..=15.0).contains(&suggestion.contrast));
    assert!((-8.0..=4.0).contains(&suggestion.whites));
    assert!((-8.0..=4.0).contains(&suggestion.blacks));
}

#[test]
fn gradient_mask_is_persisted_labeled_and_renderable() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let mask = LayerMask {
        gradient: Some(MaskGradient {
            kind: MaskGradientKind::Linear,
            start: MaskPoint { x: 0.5, y: 0.1 },
            end: MaskPoint { x: 0.5, y: 0.9 },
        }),
        inverted: true,
        feather: 0.5,
        density: 0.6,
        ..LayerMask::default()
    };
    let masked = app
        .service
        .set_layer_mask(SetLayerMaskRequest {
            project_id: state.id.clone(),
            layer_id: layer_id.clone(),
            mask: Some(mask),
        })
        .unwrap();
    let stored = masked
        .layers
        .iter()
        .find(|layer| layer.id == layer_id)
        .and_then(|layer| layer.mask.as_ref())
        .expect("gradient mask");
    assert!(stored.gradient.is_some());
    assert!(stored.inverted);
    assert_eq!(stored.feather, 0.5);
    assert_eq!(stored.density, 0.6);
    assert!(masked
        .history
        .last()
        .is_some_and(|label| label == "渐变修饰范围"));

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id,
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());
}

#[test]
fn white_balance_pick_samples_source_and_returns_in_range_gains() {
    let app = TestApp::new();
    let state = import(&app);

    let balanced = app
        .service
        .pick_white_balance(PickWhiteBalanceRequest {
            project_id: state.id.clone(),
            x: 0.5,
            y: 0.5,
        })
        .unwrap();
    assert!(balanced.sample.iter().all(|value| *value > 0));
    assert!((-100.0..=100.0).contains(&balanced.temperature));
    assert!((-100.0..=100.0).contains(&balanced.tint));

    let out_of_range = app.service.pick_white_balance(PickWhiteBalanceRequest {
        project_id: state.id,
        x: 1.4,
        y: 0.5,
    });
    assert!(out_of_range.is_err());
}

#[test]
fn layer_mask_round_trips_and_is_undoable() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let mask = LayerMask {
        strokes: vec![MaskStroke {
            points: vec![MaskPoint { x: 0.2, y: 0.3 }, MaskPoint { x: 0.4, y: 0.3 }],
            radius: 0.08,
            feather: 0.35,
            erase: false,
        }],
        auto_skin: false,
        ..LayerMask::default()
    };
    let masked = app
        .service
        .set_layer_mask(SetLayerMaskRequest {
            project_id: state.id.clone(),
            layer_id: layer_id.clone(),
            mask: Some(mask),
        })
        .unwrap();
    assert!(masked.layers.last().unwrap().mask.is_some());
    assert!(masked
        .history
        .last()
        .is_some_and(|label| label == "画笔修饰"));
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(64),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());

    let cleared = app
        .service
        .set_layer_mask(SetLayerMaskRequest {
            project_id: state.id.clone(),
            layer_id: layer_id.clone(),
            mask: None,
        })
        .unwrap();
    assert!(cleared.layers.last().unwrap().mask.is_none());
    assert!(cleared
        .history
        .last()
        .is_some_and(|label| label == "清除修饰范围"));

    let undone = app
        .service
        .undo(app_core::ProjectIdRequest {
            project_id: state.id,
        })
        .unwrap();
    assert!(undone.layers.last().unwrap().mask.is_some());
    assert_eq!(undone.layers.last().unwrap().id, layer_id);
}

#[test]
fn project_rename_delete_and_render_cache_clear() {
    let app = TestApp::new();
    let state = import(&app);
    let project_id = state.id.clone();

    let renamed = app
        .service
        .rename_project(RenameProjectRequest {
            project_id: project_id.clone(),
            name: "新的项目名".into(),
        })
        .unwrap();
    assert_eq!(renamed.name, "新的项目名");
    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert_eq!(reopened.name, "新的项目名");

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());
    let cleared = app.service.clear_render_cache().unwrap();
    assert!(cleared.deleted_files >= 1);
    assert!(!Path::new(&preview.path).exists());

    app.service
        .delete_project(app_core::ProjectIdRequest {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert!(app.service.list_projects().unwrap().is_empty());
    let missing = app
        .service
        .open_project(OpenProjectRequest { project_id })
        .unwrap_err();
    assert_eq!(missing.code, "PROJECT_NOT_FOUND");
}

#[test]
fn export_is_atomic_and_refuses_to_overwrite_original() {
    let app = TestApp::new();
    let state = import(&app);
    let sample_path = state.source.as_ref().unwrap().original_path.clone();
    let sample = std::path::PathBuf::from(&sample_path);
    let original_bytes = fs::read(&sample).unwrap();

    let out = app.dir.path().join("out.png");
    let result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 90,
                max_dimension: None,
                width: None,
                height: None,

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!((result.width, result.height), (64, 48));
    assert!(result.bytes > 0);
    assert!(out.exists());

    let proxy = app.dir.path().join("proxy.jpg");
    let result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: proxy.to_string_lossy().to_string(),
                format: ExportImageFormat::Jpeg,
                quality: 85,
                max_dimension: Some(32),
                width: None,
                height: None,

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!((result.width, result.height), (32, 24));

    let error = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id,
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: sample_path,
                format: ExportImageFormat::Png,
                quality: 90,
                max_dimension: None,
                width: None,
                height: None,

                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert_eq!(fs::read(&sample).unwrap(), original_bytes);
}

#[test]
fn committed_edits_survive_reopen_but_history_does_not() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: state.id.clone(),
            layer_id,
            color: Some(ColorParams {
                saturation: 25.0,
                ..Default::default()
            }),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: state.id.clone(),
            label: "饱和度".into(),
        })
        .unwrap();

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    drop(service);
    let reopened_service = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    let reopened = reopened_service
        .open_project(OpenProjectRequest {
            project_id: state.id,
        })
        .unwrap();
    assert_eq!(reopened.layers.len(), 2);
    assert_eq!(
        reopened.layers.last().unwrap().color.unwrap().saturation,
        25.0
    );
    assert!(reopened.history.is_empty());
    let preview = reopened_service
        .render_preview(RenderPreviewRequest {
            project_id: reopened.id,
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());
}

#[tokio::test]
async fn mock_ai_job_completes_and_applies_as_layer() {
    let app = TestApp::new();
    let state = import(&app);
    let providers = app.service.list_providers().unwrap();
    let mock = providers
        .iter()
        .find(|provider| provider.kind == ProviderKind::Mock)
        .expect("seeded mock provider");
    let models = app
        .service
        .list_provider_models(app_core::ProviderIdRequest {
            provider_id: mock.id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(models, vec!["luma-demo-v1".to_string()]);

    let response = app
        .service
        .submit_ai_job(SubmitAiJobRequest {
            project_id: state.id.clone(),
            provider_id: mock.id.clone(),
            operation: AiJobOperation::Edit,
            prompt: "黑白电影感".into(),
            strength: 60.0,
        })
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let job = loop {
        let jobs = app
            .service
            .list_ai_jobs(ListAiJobsRequest {
                project_id: state.id.clone(),
            })
            .unwrap();
        let job = jobs
            .into_iter()
            .find(|job| job.id == response.job_id)
            .expect("submitted job");
        if job.status.is_terminal() {
            break job;
        }
        assert!(Instant::now() < deadline, "AI job did not finish in time");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(job.status, AiJobStatus::Completed, "{job:?}");
    let result_path = job.result_path.clone().expect("result path");
    assert!(Path::new(&result_path).exists());
    let result_asset_id = job.result_asset_id.clone().expect("result asset id");

    let applied = app
        .service
        .apply_ai_result(ApplyAiResultRequest {
            project_id: state.id.clone(),
            job_id: response.job_id.clone(),
        })
        .unwrap();
    let ai_layer = applied.layers.last().unwrap();
    assert_eq!(ai_layer.kind, "aiResult");
    assert_eq!(ai_layer.asset_id.as_deref(), Some(result_asset_id.as_str()));
    assert!(ai_layer
        .preview_path
        .as_ref()
        .is_some_and(|path| Path::new(path).exists()));

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());

    assert!(app
        .events
        .snapshot()
        .iter()
        .any(|event| event.status == AiJobStatus::Completed));

    let duplicate = app
        .service
        .apply_ai_result(ApplyAiResultRequest {
            project_id: state.id,
            job_id: response.job_id,
        })
        .unwrap_err();
    assert_eq!(duplicate.code, "INVALID_ARGUMENT");
}

#[tokio::test]
async fn provider_config_keeps_secrets_out_of_sqlite() {
    let app = TestApp::new();
    let providers = app.service.list_providers().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].kind, ProviderKind::Mock);

    // Plain HTTP is rejected unless explicitly allowed for loopback.
    let error = app
        .service
        .save_provider_config(ProviderConfigInput {
            id: None,
            name: "远程 Provider".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: Some("http://example.com/v1".into()),
            model: Some("gpt-image-1".into()),
            allow_localhost: None,
        })
        .unwrap_err();
    assert_eq!(error.code, "PROVIDER_UNAVAILABLE");

    let provider = app
        .service
        .save_provider_config(ProviderConfigInput {
            id: None,
            name: "远程 Provider".into(),
            kind: ProviderKind::OpenAiCompatible,
            base_url: Some("https://api.example.com/v1".into()),
            model: Some("gpt-image-1".into()),
            allow_localhost: None,
        })
        .unwrap();
    assert!(!provider.has_credential);
    assert_eq!(provider.kind, ProviderKind::OpenAiCompatible);

    let secret = "sk-test-secret-value";
    let updated = app
        .service
        .set_provider_credential(SetProviderCredentialRequest {
            provider_id: provider.id.clone(),
            api_key: secret.into(),
        })
        .unwrap();
    assert!(updated.has_credential);
    assert_eq!(
        app.credentials.get(&provider.id).unwrap().as_deref(),
        Some(secret)
    );
    // Credentials live in the credential store, never in SQLite/provider JSON.
    let db_bytes = fs::read(app.dir.path().join("data").join("luma.db")).unwrap();
    assert!(!db_bytes
        .windows(secret.len())
        .any(|window| window == secret.as_bytes()));

    let cleared = app
        .service
        .set_provider_credential(SetProviderCredentialRequest {
            provider_id: provider.id.clone(),
            api_key: String::new(),
        })
        .unwrap();
    assert!(!cleared.has_credential);

    let mock = providers[0].id.clone();
    let error: ApiError = app
        .service
        .set_provider_credential(SetProviderCredentialRequest {
            provider_id: mock,
            api_key: "nope".into(),
        })
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
}

fn p3_tagged_png(app: &TestApp, name: &str, color: [u8; 3]) -> std::path::PathBuf {
    use image_core::{
        build_rgb_profile, encode_atomic_profiled, OutputFormat, ProfileTrc, DISPLAY_P3_PRIMARIES,
    };
    let profile = build_rgb_profile("Display P3", DISPLAY_P3_PRIMARIES, ProfileTrc::Srgb).unwrap();
    let mut rgba = Vec::new();
    for _ in 0..4 {
        rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
    }
    let image = DecodedImage::new(2, 2, rgba).unwrap();
    let path = app.dir.path().join(name);
    encode_atomic_profiled(&path, &image, OutputFormat::Png, 100, Some(&profile)).unwrap();
    path
}

fn export(
    app: &TestApp,
    project_id: &str,
    path: &std::path::Path,
    format: ExportImageFormat,
    color_space: ExportColorSpace,
) -> Result<app_core::ExportResult, ApiError> {
    app.service.export_image(ExportImageRequest {
        project_id: project_id.to_string(),
        job_id: None,
        options: ExportOptions {
            watermark: None,
            path: path.to_string_lossy().to_string(),
            format,
            quality: 100,
            max_dimension: None,
            width: None,
            height: None,
            color_space,
            metadata: ExportMetadata::Keep,
            bit_depth: ExportBitDepth::Eight,
        },
    })
}

/// Little-endian EXIF block with orientation, a GPS pointer and a thumbnail
/// pair inside the Exif sub-IFD, mirroring what cameras produce.
fn exif_with_gps_and_thumbnail() -> Vec<u8> {
    let ifd0 = 8usize;
    let exif_ifd = ifd0 + 2 + 4 * 12 + 4;
    let gps_ifd = exif_ifd + 2 + 2 * 12 + 4;
    let entries = [
        (0x0112u16, 6u32),        // orientation: rotated 90 degrees
        (0x8825, gps_ifd as u32), // GPSInfo pointer to a real (empty) IFD
        (0x8769, 0),              // ExifIFD pointer, filled below
        (0x0131, 7),              // software, must survive
    ];
    let mut out = Vec::new();
    out.extend_from_slice(b"II*\0");
    out.extend_from_slice(&(ifd0 as u32).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, value) in entries {
        let value = if tag == 0x8769 {
            exif_ifd as u32
        } else {
            value
        };
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    let sub = [0x0201u16, 0x0202];
    out.extend_from_slice(&(sub.len() as u16).to_le_bytes());
    for tag in sub {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&32u32.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    // An empty GPS IFD so the pointer in IFD0 resolves.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

fn exported_exif(path: &std::path::Path) -> Option<Vec<u8>> {
    image_core::read_source_exif(path).unwrap()
}

fn tag_value(tiff: &[u8], tag: u16) -> Option<u32> {
    let ifd = u32::from_le_bytes(tiff[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(tiff[ifd..ifd + 2].try_into().unwrap()) as usize;
    for index in 0..count {
        let entry = ifd + 2 + index * 12;
        if u16::from_le_bytes(tiff[entry..entry + 2].try_into().unwrap()) == tag {
            return Some(u32::from_le_bytes(
                tiff[entry + 8..entry + 12].try_into().unwrap(),
            ));
        }
    }
    None
}

#[test]
fn metadata_policies_keep_privacy_and_none_behave_differently() {
    let app = TestApp::new();
    let exif = exif_with_gps_and_thumbnail();
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            rgba.extend_from_slice(&[(x * 30) as u8, (y * 30) as u8, 90, 255]);
        }
    }
    let source_image = DecodedImage::new(8, 8, rgba).unwrap();
    let source_path = app.dir.path().join("with-exif.png");
    image_core::encode_atomic_image(
        &source_path,
        &image_core::RasterImage::Eight(source_image.clone()),
        image_core::OutputFormat::Png,
        100,
        &image_core::EncodeOptions {
            icc: None,
            exif: Some(&exif),
        },
    )
    .unwrap();
    assert!(image_core::has_gps(&exif));

    let state = app
        .service
        .import_photo(ImportPhotoRequest {
            path: source_path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: None,
        })
        .unwrap();

    let export_with = |name: &str, format: ExportImageFormat, metadata: ExportMetadata| {
        let path = app.dir.path().join(name);
        let result = app
            .service
            .export_image(ExportImageRequest {
                project_id: state.id.clone(),
                job_id: None,
                options: ExportOptions {
                    watermark: None,
                    path: path.to_string_lossy().to_string(),
                    format,
                    quality: 100,
                    max_dimension: None,
                    width: None,
                    height: None,
                    color_space: ExportColorSpace::Srgb,
                    metadata,
                    bit_depth: ExportBitDepth::Eight,
                },
            })
            .unwrap();
        (path, result)
    };

    let (kept_path, kept) = export_with("kept.png", ExportImageFormat::Png, ExportMetadata::Keep);
    assert!(kept.metadata_written);
    assert!(
        kept.keeps_location,
        "Keep must warn that GPS is still there"
    );
    let written = exported_exif(&kept_path).expect("png keeps exif");
    let body = &written[written.windows(2).position(|p| p == b"II").unwrap()..];
    assert_eq!(tag_value(body, 0x0112), Some(1), "orientation is baked in");
    assert_eq!(tag_value(body, 0x0131), Some(7), "unrelated tags survive");
    assert!(tag_value(body, 0x8769).is_some());
    let exif_ifd = tag_value(body, 0x8769).unwrap() as usize;
    let sub_count = u16::from_le_bytes(body[exif_ifd..exif_ifd + 2].try_into().unwrap());
    assert_eq!(sub_count, 0, "the stale thumbnail is dropped");

    let (private_path, private) = export_with(
        "private.png",
        ExportImageFormat::Png,
        ExportMetadata::Privacy,
    );
    assert!(private.metadata_written);
    assert!(!private.keeps_location);
    let body = exported_exif(&private_path).unwrap();
    let body = &body[body.windows(2).position(|p| p == b"II").unwrap()..];
    assert_eq!(tag_value(body, 0x8825), None, "gps pointer is removed");
    assert_eq!(tag_value(body, 0x0131), Some(7));

    let (stripped_path, stripped) =
        export_with("stripped.png", ExportImageFormat::Png, ExportMetadata::None);
    assert!(!stripped.metadata_written);
    assert!(exported_exif(&stripped_path).is_none());

    // TIFF gets its EXIF injected after encoding, so it must report it.
    let (tiff_path, tiff) = export_with("wide.tif", ExportImageFormat::Tiff, ExportMetadata::Keep);
    assert!(tiff.metadata_written);
    assert!(
        tiff.keeps_location,
        "Keep must warn that GPS is still there"
    );
    assert!(image_core::tiff_file_has_exif(&tiff_path));
    let tiff_body = fs::read(&tiff_path).unwrap();
    let tiff_ifd0 = u32::from_le_bytes(tiff_body[4..8].try_into().unwrap()) as usize;
    let tiff_exif = tiff_exif_ifd(&tiff_body).expect("IFD0 points at an Exif IFD");
    assert_eq!(
        tiff_ifd_value(&tiff_body, tiff_ifd0, 0x0112),
        None,
        "orientation is baked into the pixels, not re-tagged"
    );
    assert_eq!(
        tiff_ifd_value(&tiff_body, tiff_ifd0, 0x0131),
        Some(7),
        "software tag survives into IFD0"
    );
    assert!(
        tiff_ifd_value(&tiff_body, tiff_ifd0, 0x8825).is_some(),
        "Keep preserves the GPS pointer"
    );
    assert_eq!(
        tiff_ifd_value(&tiff_body, tiff_exif, 0x0201),
        None,
        "the stale thumbnail was dropped before embedding"
    );

    let (private_tiff, private_result) = export_with(
        "private.tif",
        ExportImageFormat::Tiff,
        ExportMetadata::Privacy,
    );
    assert!(private_result.metadata_written);
    assert!(!private_result.keeps_location);
    let body = fs::read(&private_tiff).unwrap();
    let ifd0 = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    assert_eq!(
        tiff_ifd_value(&body, ifd0, 0x8825),
        None,
        "gps pointer is removed"
    );

    // ICC now survives TIFF as well: `image` writes the 34675 tag natively.
    let p3_tiff = app.dir.path().join("p3.tif");
    let p3_result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: p3_tiff.to_string_lossy().to_string(),
                format: ExportImageFormat::Tiff,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::DisplayP3,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!(p3_result.color_space, "Display P3");
    assert!(
        image_core::tiff_file_has_tag(&p3_tiff, 0x8773),
        "the TIFF carries an ICC profile tag"
    );
}

/// Offset of the Exif IFD referenced by IFD0, if any.
fn tiff_exif_ifd(tiff: &[u8]) -> Option<usize> {
    let little = &tiff[0..2] == b"II";
    let read_u16_at = |offset: usize| -> Option<u16> {
        let bytes = tiff.get(offset..offset + 2)?;
        Some(if little {
            u16::from_le_bytes([bytes[0], bytes[1]])
        } else {
            u16::from_be_bytes([bytes[0], bytes[1]])
        })
    };
    let read_u32_at = |offset: usize| -> Option<u32> {
        let bytes = tiff.get(offset..offset + 4)?;
        Some(if little {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        })
    };
    let ifd0 = read_u32_at(4)? as usize;
    let count = read_u16_at(ifd0)? as usize;
    for index in 0..count {
        let entry = ifd0 + 2 + index * 12;
        if read_u16_at(entry)? == 0x8769 {
            return Some(read_u32_at(entry + 8)? as usize);
        }
    }
    None
}

/// First LONG value of `tag` inside an IFD (the tags used here are SHORT/LONG).
fn tiff_ifd_value(tiff: &[u8], ifd: usize, tag: u16) -> Option<u32> {
    let little = &tiff[0..2] == b"II";
    let read_u16_at = |offset: usize| -> Option<u16> {
        let bytes = tiff.get(offset..offset + 2)?;
        Some(if little {
            u16::from_le_bytes([bytes[0], bytes[1]])
        } else {
            u16::from_be_bytes([bytes[0], bytes[1]])
        })
    };
    let read_u32_at = |offset: usize| -> Option<u32> {
        let bytes = tiff.get(offset..offset + 4)?;
        Some(if little {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        })
    };
    let count = read_u16_at(ifd)? as usize;
    for index in 0..count {
        let entry = ifd + 2 + index * 12;
        if read_u16_at(entry)? != tag {
            continue;
        }
        let typ = read_u16_at(entry + 2)?;
        return match typ {
            3 => read_u16_at(entry + 8).map(u32::from),
            _ => read_u32_at(entry + 8),
        };
    }
    None
}

#[test]
fn sixteen_bit_export_uses_the_linear_pipeline_and_rejects_jpeg() {
    let app = TestApp::new();
    let state = import(&app);

    let png = app.dir.path().join("deep.png");
    let result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: png.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: None,
                width: Some(24),
                height: Some(18),
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Sixteen,
            },
        })
        .unwrap();
    assert_eq!(result.bit_depth, 16);
    assert_eq!(
        result.backend, "cpu",
        "16-bit must not come from the 8-bit GPU path"
    );
    assert_eq!((result.width, result.height), (24, 18));
    assert_eq!(
        image_core::probe_color_type(&png).unwrap(),
        "rgba16",
        "png export must keep 16-bit samples"
    );

    let jpg = app.dir.path().join("deep.jpg");
    let error: ApiError = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: jpg.to_string_lossy().to_string(),
                format: ExportImageFormat::Jpeg,
                quality: 90,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Sixteen,
            },
        })
        .unwrap_err();
    assert_eq!(error.code, "BIT_DEPTH_UNSUPPORTED");
    assert!(!jpg.exists());

    let tiff = app.dir.path().join("deep.tif");
    let tiff_result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id,
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: tiff.to_string_lossy().to_string(),
                format: ExportImageFormat::Tiff,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Sixteen,
            },
        })
        .unwrap();
    assert_eq!(tiff_result.bit_depth, 16);
}

fn sample_portrait(app: &TestApp) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("apps/desktop/public/samples/portrait.jpg");
    assert!(path.exists(), "the bundled sample portrait is missing");
    let target = app.dir.path().join("portrait.jpg");
    fs::copy(&path, &target).unwrap();
    target
}

#[test]
fn region_analysis_finds_faces_and_survives_reopen_through_the_mask() {
    let app = TestApp::new();
    let portrait = sample_portrait(&app);
    let state = app
        .service
        .import_photo(ImportPhotoRequest {
            path: portrait.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: None,
        })
        .unwrap();

    let regions = app
        .service
        .analyze_portrait_regions(ProjectIdRequest {
            project_id: state.id.clone(),
        })
        .unwrap();
    assert!(regions.available);
    assert_eq!(
        regions.faces.len(),
        1,
        "the sample portrait holds one clear face"
    );
    let face = &regions.faces[0];
    assert!(face.score > 0.4, "score {}", face.score);
    for kind in ["leftEye", "rightEye", "skin", "mouth"] {
        assert!(
            face.regions.iter().any(|region| region.kind == kind),
            "missing {kind}"
        );
    }
    for region in &face.regions {
        assert!((0.0..=1.0).contains(&region.center_x));
        assert!((0.0..=1.0).contains(&region.center_y));
        assert!(region.radius_x > 0.0 && region.radius_x < 0.6);
        assert!((0.0..=1.0).contains(&region.confidence));
    }
    let eye = face
        .regions
        .iter()
        .find(|region| region.kind == "leftEye")
        .unwrap();
    let right_eye = face
        .regions
        .iter()
        .find(|region| region.kind == "rightEye")
        .unwrap();
    assert!(
        (eye.center_y - right_eye.center_y).abs() < 0.03,
        "both eyes share a line"
    );

    // A second call must be served from the session cache.
    let again = app
        .service
        .analyze_portrait_regions(ProjectIdRequest {
            project_id: state.id.clone(),
        })
        .unwrap();
    assert_eq!(again.faces, regions.faces);

    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let project_id = state.id.clone();
    app.service
        .set_layer_mask(SetLayerMaskRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            mask: Some(LayerMask {
                strokes: Vec::new(),
                auto_skin: false,
                regions: vec![MaskRegion {
                    kind: "leftEye".into(),
                    center_x: eye.center_x,
                    center_y: eye.center_y,
                    radius_x: eye.radius_x,
                    radius_y: eye.radius_y,
                    angle: eye.angle,
                    feather: eye.feather,
                    strength: 1.0,
                }],
                ..Default::default()
            }),
        })
        .unwrap();
    let committed = app
        .service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "眼睛范围".into(),
        })
        .unwrap();
    assert_eq!(
        committed
            .layers
            .last()
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .regions
            .len(),
        1
    );
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(64),
            ignore_crop: false,
        })
        .unwrap();
    assert_eq!(preview.backend, "cpu", "masks force the CPU path");

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    drop(service);
    let reopened = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    let document = reopened
        .open_project(OpenProjectRequest { project_id })
        .unwrap();
    let mask = document.layers.last().unwrap().mask.as_ref().unwrap();
    assert_eq!(mask.regions.len(), 1);
    assert_eq!(mask.regions[0].kind, "leftEye");
    assert!((mask.regions[0].center_x - eye.center_x).abs() < 1e-5);
}

#[test]
fn wide_gamut_import_remaps_pixels_and_source_export_restores_them() {
    let app = TestApp::new();
    // An in-gamut warm grey. A saturated orange would clip on the way into
    // sRGB and could never come back.
    let path = p3_tagged_png(&app, "p3.png", [140, 120, 90]);
    let state = app
        .service
        .import_photo(ImportPhotoRequest {
            path: path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: None,
        })
        .unwrap();
    let source = state.source.clone().expect("source state");
    assert_eq!(source.color_space, "Display P3");
    assert!(source.has_source_profile);

    // Import moved the pixels into the working space, so the cached preview is
    // no longer the raw code values from the file.
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(8),
            ignore_crop: false,
        })
        .unwrap();
    let rendered = image_core::decode_image_from_path(std::path::Path::new(&preview.path)).unwrap();
    let remapped = rendered.pixel(0, 0).unwrap();
    assert_ne!(&remapped[..3], &[140, 120, 90]);

    let out = app.dir.path().join("source-space.png");
    let result = export(
        &app,
        &state.id,
        &out,
        ExportImageFormat::Png,
        ExportColorSpace::Source,
    )
    .unwrap();
    assert_eq!(result.color_space, "原图配置");

    let round = image_core::decode_source_from_path(&out).unwrap();
    assert_eq!(round.source_gamut, image_core::GamutLabel::DisplayP3);
    let restored = round.image.pixel(0, 0).unwrap();
    for (got, want) in restored[..3].iter().zip([140u8, 120, 90].iter()) {
        assert!(
            (*got as i16 - *want as i16).abs() <= 6,
            "two 8-bit remaps (P3 -> sRGB -> P3) only lose rounding: {restored:?}"
        );
    }
}

#[test]
fn display_p3_export_tags_the_file_and_widens_the_values() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("p3-out.png");
    let result = export(
        &app,
        &state.id,
        &out,
        ExportImageFormat::Png,
        ExportColorSpace::DisplayP3,
    )
    .unwrap();
    assert_eq!(result.color_space, "Display P3");
    let decoded = image_core::decode_source_from_path(&out).unwrap();
    assert_eq!(decoded.source_gamut, image_core::GamutLabel::DisplayP3);
    // The same picture stored as P3 must not keep sRGB code values.
    let plain = image_core::decode_image_from_path(&out).unwrap();
    let original = image_core::decode_image_from_path(&app.sample_path()).unwrap();
    let mut changed = false;
    for index in (0..original.rgba8.len()).step_by(4) {
        if original.rgba8[index] != plain.rgba8[index] {
            changed = true;
            break;
        }
    }
    assert!(changed, "P3 export should rewrite code values");
}

#[test]
fn srgb_export_stays_untagged_and_tiff_carries_wide_gamut() {
    let app = TestApp::new();
    let state = import(&app);

    let png = app.dir.path().join("plain.png");
    let result = export(
        &app,
        &state.id,
        &png,
        ExportImageFormat::Png,
        ExportColorSpace::Srgb,
    )
    .unwrap();
    assert_eq!(result.color_space, "sRGB");
    let decoded = image_core::decode_source_from_path(&png).unwrap();
    assert_eq!(decoded.source_gamut, image_core::GamutLabel::Srgb);
    let original = image_core::decode_image_from_path(&app.sample_path()).unwrap();
    assert_eq!(decoded.image.rgba8, original.rgba8);

    let tiff = app.dir.path().join("wide.tif");
    let result = export(
        &app,
        &state.id,
        &tiff,
        ExportImageFormat::Tiff,
        ExportColorSpace::DisplayP3,
    )
    .unwrap();
    assert_eq!(result.color_space, "Display P3");
    assert!(image_core::tiff_file_has_tag(&tiff, 0x8773));
    let decoded = image_core::decode_source_from_path(&tiff).unwrap();
    assert_eq!(
        decoded.source_gamut,
        image_core::GamutLabel::DisplayP3,
        "the profile is readable back from the TIFF"
    );
}

#[test]
fn imported_lut_survives_reopen_and_drives_the_cpu_renderer() {
    let app = TestApp::new();
    let state = import(&app);
    let cube_path = app.dir.path().join("warm.cube");
    fs::write(
        &cube_path,
        "TITLE \"warm\"\nLUT_1D_SIZE 3\n0 0 0\n0.9 0.5 0.2\n1 1 1\n",
    )
    .unwrap();

    let lut = app
        .service
        .import_lut(ImportLutRequest {
            project_id: state.id.clone(),
            path: Some(cube_path.to_string_lossy().to_string()),
            file_name: None,
            data_base64: None,
        })
        .unwrap();
    assert_eq!(lut.dimension, "1D");
    assert_eq!(lut.size, 3);
    assert_eq!(lut.display_name, "warm.cube");
    let asset_id = uuid::Uuid::parse_str(&lut.asset_id).unwrap();

    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: state.id.clone(),
            layer_id,
            color: Some(ColorParams {
                lut_asset_id: Some(asset_id),
                lut_strength: 0.6,
                ..Default::default()
            }),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    let committed = app
        .service
        .commit_changes(CommitChangesRequest {
            project_id: state.id.clone(),
            label: "LUT 风格".into(),
        })
        .unwrap();
    assert_eq!(committed.schema_version, ::document::CURRENT_SCHEMA_VERSION);
    let color = committed.layers.last().unwrap().color.unwrap();
    assert_eq!(color.lut_asset_id, Some(asset_id));
    assert!((color.lut_strength - 0.6).abs() < 1e-6);

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: committed.id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert_eq!(preview.backend, "cpu");

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    drop(service);
    let reopened_service = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    let reopened = reopened_service
        .open_project(OpenProjectRequest {
            project_id: committed.id.clone(),
        })
        .unwrap();
    assert_eq!(
        reopened.layers.last().unwrap().color.unwrap().lut_asset_id,
        Some(asset_id)
    );
    let preview = reopened_service
        .render_preview(RenderPreviewRequest {
            project_id: reopened.id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert!(Path::new(&preview.path).exists());
}

#[test]
fn lut_import_rejects_bad_files() {
    let app = TestApp::new();
    let state = import(&app);

    let not_a_cube = app.dir.path().join("notes.txt");
    fs::write(&not_a_cube, "LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").unwrap();
    let error: ApiError = app
        .service
        .import_lut(ImportLutRequest {
            project_id: state.id.clone(),
            path: Some(not_a_cube.to_string_lossy().to_string()),
            file_name: None,
            data_base64: None,
        })
        .unwrap_err();
    assert_eq!(error.code, "LUT_INVALID");

    let broken = app.dir.path().join("broken.cube");
    fs::write(&broken, "LUT_3D_SIZE 2\n0 0 0\n").unwrap();
    let error: ApiError = app
        .service
        .import_lut(ImportLutRequest {
            project_id: state.id.clone(),
            path: Some(broken.to_string_lossy().to_string()),
            file_name: None,
            data_base64: None,
        })
        .unwrap_err();
    assert_eq!(error.code, "LUT_INVALID");

    let error: ApiError = app
        .service
        .import_lut(ImportLutRequest {
            project_id: state.id.clone(),
            path: None,
            file_name: None,
            data_base64: None,
        })
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");

    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        "LUT_1D_SIZE 2\n0 0 0\n1 1 1\n",
    );
    let lut = app
        .service
        .import_lut(ImportLutRequest {
            project_id: state.id,
            path: None,
            file_name: Some("mono.cube".into()),
            data_base64: Some(format!("data:text/plain;base64,{encoded}")),
        })
        .unwrap();
    assert_eq!(lut.dimension, "1D");
}

#[test]
fn render_previews_are_pruned_as_revisions_advance() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = state.layers.last().unwrap().id.clone();
    let project_id = state.id.clone();

    for i in 0..8u32 {
        app.service
            .update_layer_params(UpdateLayerParamsRequest {
                project_id: project_id.clone(),
                layer_id: layer_id.clone(),
                color: Some(ColorParams {
                    exposure: i as f32 * 0.05,
                    ..Default::default()
                }),
                portrait: None,
                retouch: None,
            })
            .unwrap();
        app.service
            .commit_changes(CommitChangesRequest {
                project_id: project_id.clone(),
                label: format!("曝光 {i}"),
            })
            .unwrap();
        app.service
            .render_preview(RenderPreviewRequest {
                project_id: project_id.clone(),
                max_dimension: Some(64),
                ignore_crop: false,
            })
            .unwrap();
    }

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(64),
            ignore_crop: false,
        })
        .unwrap();
    let dir = std::path::Path::new(&preview.path)
        .parent()
        .unwrap()
        .to_path_buf();
    let renders: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(|name| name.to_string()))
        .filter(|name| name.starts_with("render-"))
        .collect();
    assert!(renders.len() <= 2, "eight revisions left {renders:?}");
    let current = std::path::Path::new(&preview.path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        renders.contains(&current),
        "the preview the canvas is showing must never be pruned"
    );
    assert!(
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("source-")),
        "source previews are not render cache and must survive"
    );
}

#[test]
fn concurrent_edits_to_two_projects_both_persist() {
    let app = TestApp::new();
    let mut targets = Vec::new();
    for _ in 0..2 {
        let state = import(&app);
        let state = app
            .service
            .add_layer(AddLayerRequest {
                project_id: state.id.clone(),
                kind: AddLayerKind::Color,
                name: None,
                portrait: None,
                mask: None,
            })
            .unwrap();
        let layer_id = state.layers.last().unwrap().id.clone();
        targets.push((state.id, layer_id));
    }

    let barrier = Arc::new(Barrier::new(targets.len()));
    let handles: Vec<_> = targets
        .iter()
        .cloned()
        .map(|(project_id, layer_id)| {
            let service = app.service.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for i in 0..12 {
                    service
                        .update_layer_params(UpdateLayerParamsRequest {
                            project_id: project_id.clone(),
                            layer_id: layer_id.clone(),
                            color: Some(ColorParams {
                                exposure: i as f32 * 0.1,
                                ..Default::default()
                            }),
                            portrait: None,
                            retouch: None,
                        })
                        .unwrap();
                    service
                        .commit_changes(CommitChangesRequest {
                            project_id: project_id.clone(),
                            label: format!("并发 {i}"),
                        })
                        .unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    drop(service);
    let reopened = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    for (project_id, _) in &targets {
        let doc = reopened
            .open_project(OpenProjectRequest {
                project_id: project_id.clone(),
            })
            .unwrap();
        let layer = doc
            .layers
            .iter()
            .find(|layer| layer.kind == "color")
            .expect("the colour layer survived the round trip");
        assert!(
            (layer.color.as_ref().unwrap().exposure - 1.1).abs() < 1e-5,
            "lost or reordered writes: {:?}",
            layer.color.as_ref().unwrap().exposure
        );
    }
}

#[test]
fn retouch_layer_persists_and_forces_the_cpu_path() {
    let app = TestApp::new();
    let state = import(&app);
    let state = app
        .service
        .add_layer(AddLayerRequest {
            project_id: state.id.clone(),
            kind: AddLayerKind::Retouch,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer = state.layers.last().unwrap();
    assert_eq!(layer.kind, "retouch");
    let layer_id = layer.id.clone();
    let project_id = state.id.clone();

    let params = RetouchParams {
        spots: vec![RetouchSpot {
            mode: RetouchMode::Clone,
            x: 0.6,
            y: 0.5,
            radius: 0.1,
            source_x: Some(0.3),
            source_y: Some(0.5),
            hardness: 0.8,
            opacity: 1.0,
        }],
        ..Default::default()
    };
    let state = app
        .service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: project_id.clone(),
            layer_id: layer_id.clone(),
            color: None,
            portrait: None,
            retouch: Some(params),
        })
        .unwrap();
    assert_eq!(
        state
            .layers
            .last()
            .unwrap()
            .retouch
            .as_ref()
            .unwrap()
            .spots
            .len(),
        1
    );
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: project_id.clone(),
            label: "克隆图章".into(),
        })
        .unwrap();
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: project_id.clone(),
            max_dimension: Some(64),
            ignore_crop: false,
        })
        .unwrap();
    assert_eq!(
        preview.backend, "cpu",
        "retouch layers must stay on the CPU path"
    );

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    drop(service);
    let reopened = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    let document = reopened
        .open_project(OpenProjectRequest { project_id })
        .unwrap();
    let retouch = document.layers.last().unwrap().retouch.as_ref().unwrap();
    assert_eq!(retouch.spots.len(), 1);
    assert_eq!(retouch.spots[0].mode, RetouchMode::Clone);
    assert_eq!(retouch.spots[0].source_x, Some(0.3));
}

#[test]
fn ai_denoise_runs_a_local_onnx_model_and_adds_a_layer() {
    let app = TestApp::new();
    let state = import(&app);
    let model_path = app.dir.path().join("identity.onnx");
    fs::write(&model_path, ai_denoise::testing::identity_onnx()).unwrap();

    let state = app
        .service
        .run_ai_denoise(AiDenoiseRequest {
            project_id: state.id.clone(),
            model_path: model_path.to_string_lossy().to_string(),
            strength: Some(1.0),
        })
        .unwrap();
    let layer = state.layers.last().unwrap();
    assert_eq!(layer.kind, "aiResult");
    assert_eq!(layer.name, "AI 降噪");
    assert!(
        layer.asset_id.is_some(),
        "the denoise result is a managed asset"
    );

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(64),
            ignore_crop: false,
        })
        .unwrap();
    assert!(preview.width > 0 && preview.height > 0);
    assert!(
        matches!(preview.backend.as_str(), "cpu" | "wgpu"),
        "unexpected backend {}",
        preview.backend
    );

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    let project_id = state.id.clone();
    drop(service);
    let reopened = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    let document = reopened
        .open_project(OpenProjectRequest { project_id })
        .unwrap();
    let layer = document.layers.last().unwrap();
    assert_eq!(layer.kind, "aiResult");
    assert_eq!(layer.name, "AI 降噪");
    assert!(layer.asset_id.is_some());
}

#[test]
#[ignore = "requires the bundled Real-ESRGAN runtime and a Vulkan GPU"]
fn photo_hq_denoise_adds_a_layer() {
    let app = TestApp::new();
    let state = import(&app);
    let engine = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/src-tauri/resources/realesrgan");
    let capability = app.service.ai_denoise_capability(
        AiCapabilityRequest {
            onnx_model_path: None,
        },
        Some(&engine),
    );
    assert!(
        capability.photo_hq.available,
        "photo-hq probe failed: {:?}",
        capability.photo_hq.reason
    );
    assert!(!capability.onnx.available);
    let state = app
        .service
        .run_photo_hq_denoise(
            PhotoHqDenoiseRequest {
                project_id: state.id.clone(),
                strength: Some(0.8),
                job_id: None,
            },
            &engine,
        )
        .unwrap();
    let layer = state.layers.last().unwrap();
    assert_eq!(layer.kind, "aiResult");
    assert_eq!(layer.name, "AI 降噪 · Photo-HQ");
    assert!(layer.asset_id.is_some());
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: state.id.clone(),
            max_dimension: Some(64),
            ignore_crop: false,
        })
        .unwrap();
    assert!(preview.width > 0 && preview.height > 0);

    let TestApp {
        service,
        credentials,
        events: _,
        dir,
    } = app;
    let project_id = state.id.clone();
    drop(service);
    let reopened = AppService::with_event_sink_and_credentials(
        AppPaths::new(dir.path().join("data"), dir.path().join("cache")),
        Arc::new(NoopEventSink),
        credentials,
    )
    .unwrap();
    let document = reopened
        .open_project(OpenProjectRequest { project_id })
        .unwrap();
    let layer = document.layers.last().unwrap();
    assert_eq!(layer.kind, "aiResult");
    assert_eq!(layer.name, "AI 降噪 · Photo-HQ");
}

#[test]
fn large_source_preview_uses_a_scaled_source_proxy() {
    let app = TestApp::new();
    let path = app.dir.path().join("wide.png");
    let width = 2400u32;
    let height = 1600u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            if x < width / 2 {
                rgba.extend_from_slice(&[220, 20, 20, 255]);
            } else {
                rgba.extend_from_slice(&[20, 20, 220, 255]);
            }
        }
    }
    let image = DecodedImage::new(width, height, rgba).unwrap();
    encode_atomic(&path, &image, OutputFormat::Png, 100).unwrap();
    let state = app
        .service
        .import_photo(ImportPhotoRequest {
            path: path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: None,
        })
        .unwrap();
    let project_id = state.id.clone();
    app.service
        .set_crop(SetCropRequest {
            project_id: project_id.clone(),
            crop: CropRect {
                x: width / 2,
                y: 0,
                width: width / 2,
                height,
                angle: 0.0,
            },
        })
        .unwrap();

    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id,
            max_dimension: Some(600),
            ignore_crop: false,
        })
        .unwrap();
    // The 1600px source proxy is cropped to its right half, then rendered at
    // 450×600. A missed crop scale would expose transparent or red pixels.
    assert_eq!((preview.width, preview.height), (450, 600));
    let decoded = image_core::decode_image_from_path(Path::new(&preview.path)).unwrap();
    let center = decoded
        .rgba8
        .chunks_exact(4)
        .nth((decoded.width * decoded.height / 2 + decoded.width / 2) as usize)
        .unwrap();
    assert!(center[2] > 180, "expected blue, got {center:?}");
    assert!(center[0] < 80, "expected blue, got {center:?}");
}

#[test]
fn imports_can_share_a_collection_and_collections_group_photos() {
    let app = TestApp::new();
    let first_path = app.sample_path();
    let first = app
        .service
        .import_photo(ImportPhotoRequest {
            path: first_path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: Some("婚礼修图".to_string()),
        })
        .unwrap();
    assert_eq!(first.collection_name, "婚礼修图");

    let second_path = app.dir.path().join("second.png");
    write_sample(&second_path);
    let second = app
        .service
        .import_photo(ImportPhotoRequest {
            path: second_path.to_string_lossy().to_string(),
            collection_id: Some(first.collection_id.clone()),
            collection_name: None,
        })
        .unwrap();
    assert_eq!(second.collection_id, first.collection_id);
    assert_eq!(second.collection_name, "婚礼修图");

    let third_path = app.dir.path().join("third.png");
    write_sample(&third_path);
    let third = app
        .service
        .import_photo(ImportPhotoRequest {
            path: third_path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: None,
        })
        .unwrap();
    assert_ne!(third.collection_id, first.collection_id);

    let collections = app.service.list_collections().unwrap();
    assert_eq!(collections.len(), 2);
    let shared = collections
        .iter()
        .find(|collection| collection.id == first.collection_id)
        .unwrap();
    assert_eq!(shared.name, "婚礼修图");
    assert_eq!(shared.photo_count, 2);
    assert_eq!(shared.latest_project_id, second.id);
    assert!(shared.cover_thumbnail_path.is_some());

    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: second.id.clone(),
        })
        .unwrap();
    assert_eq!(reopened.collection_id, first.collection_id);
    assert_eq!(reopened.collection_name, "婚礼修图");

    let missing = app.service.import_photo(ImportPhotoRequest {
        path: first_path.to_string_lossy().to_string(),
        collection_id: Some(Uuid::new_v4().to_string()),
        collection_name: None,
    });
    assert!(missing.is_err());
}

#[test]
fn renaming_a_collection_updates_every_member_and_the_open_session() {
    let app = TestApp::new();
    let first_path = app.sample_path();
    let first = app
        .service
        .import_photo(ImportPhotoRequest {
            path: first_path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: Some("旧照片集".to_string()),
        })
        .unwrap();
    let second_path = app.dir.path().join("rename-second.png");
    write_sample(&second_path);
    let second = app
        .service
        .import_photo(ImportPhotoRequest {
            path: second_path.to_string_lossy().to_string(),
            collection_id: Some(first.collection_id.clone()),
            collection_name: None,
        })
        .unwrap();
    let revision_before = app
        .service
        .open_project(OpenProjectRequest {
            project_id: second.id.clone(),
        })
        .unwrap()
        .revision;

    app.service
        .rename_collection(RenameCollectionRequest {
            collection_id: first.collection_id.clone(),
            name: "新照片集".to_string(),
        })
        .unwrap();

    let summaries = app.service.list_projects().unwrap();
    for project in summaries
        .iter()
        .filter(|project| project.collection_id == first.collection_id)
    {
        assert_eq!(project.collection_name, "新照片集");
    }
    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: second.id.clone(),
        })
        .unwrap();
    assert_eq!(reopened.collection_name, "新照片集");
    assert_eq!(
        reopened.revision, revision_before,
        "renaming a photo set must not invalidate preview caches"
    );
    assert!(app
        .service
        .rename_collection(RenameCollectionRequest {
            collection_id: Uuid::new_v4().to_string(),
            name: "不存在".to_string(),
        })
        .is_err());
}

#[test]
fn syncing_colour_adjustments_replaces_the_target_colour_stack() {
    let app = TestApp::new();
    let source_path = app.sample_path();
    let source = app
        .service
        .import_photo(ImportPhotoRequest {
            path: source_path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: Some("婚礼修图".to_string()),
        })
        .unwrap();
    let target_path = app.dir.path().join("sync-target.png");
    write_sample(&target_path);
    let target = app
        .service
        .import_photo(ImportPhotoRequest {
            path: target_path.to_string_lossy().to_string(),
            collection_id: Some(source.collection_id.clone()),
            collection_name: None,
        })
        .unwrap();

    let source = app
        .service
        .add_layer(AddLayerRequest {
            project_id: source.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let layer_id = source.layers.last().unwrap().id.clone();
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: source.id.clone(),
            layer_id: layer_id.clone(),
            color: Some(ColorParams {
                exposure: 0.8,
                contrast: 18.0,
                temperature: -12.0,
                vignette: -25.0,
                ..Default::default()
            }),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    app.service
        .set_layer_mask(SetLayerMaskRequest {
            project_id: source.id.clone(),
            layer_id,
            mask: Some(LayerMask {
                gradient: Some(MaskGradient {
                    kind: MaskGradientKind::Linear,
                    start: MaskPoint { x: 0.1, y: 0.2 },
                    end: MaskPoint { x: 0.9, y: 0.8 },
                }),
                feather: 0.3,
                ..Default::default()
            }),
        })
        .unwrap();
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: source.id.clone(),
            label: "调色".into(),
        })
        .unwrap();

    let result = app
        .service
        .sync_color_adjustments(SyncColorAdjustmentsRequest {
            source_project_id: source.id.clone(),
            project_ids: vec![target.id.clone(), target.id.clone(), source.id.clone()],
        })
        .unwrap();
    assert_eq!(
        result.synced, 1,
        "duplicates and the source itself are skipped"
    );
    assert!(result.failed.is_empty());

    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: target.id.clone(),
        })
        .unwrap();
    let colours: Vec<_> = reopened
        .layers
        .iter()
        .filter(|layer| layer.kind == "color")
        .collect();
    assert_eq!(
        colours.len(),
        1,
        "one colour layer is created on the target"
    );
    let color = colours[0].color.unwrap();
    assert!((color.exposure - 0.8).abs() < 1e-6);
    assert!((color.contrast - 18.0).abs() < 1e-6);
    assert!((color.temperature + 12.0).abs() < 1e-6);
    assert!((color.vignette + 25.0).abs() < 1e-6);
    let mask = colours[0]
        .mask
        .as_ref()
        .expect("the local mask travels with the layer");
    assert!((mask.feather - 0.3).abs() < 1e-6);
    assert_eq!(
        mask.gradient.as_ref().map(|gradient| gradient.start),
        Some(MaskPoint { x: 0.1, y: 0.2 })
    );
    assert!(reopened.can_undo, "the sync is undoable on the target");
    assert_eq!(
        reopened
            .layers
            .iter()
            .filter(|layer| layer.kind == "base")
            .count(),
        1,
        "the original photo layer is untouched"
    );
}

#[test]
fn syncing_colour_adjustments_rejects_a_neutral_source_and_skips_other_sets() {
    let app = TestApp::new();
    let source = import(&app);
    let error = app
        .service
        .sync_color_adjustments(SyncColorAdjustmentsRequest {
            source_project_id: source.id.clone(),
            project_ids: vec![source.id.clone()],
        })
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");

    let source = app
        .service
        .add_layer(AddLayerRequest {
            project_id: source.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    let unrelated = import(&app);
    assert_ne!(unrelated.collection_id, source.collection_id);
    let result = app
        .service
        .sync_color_adjustments(SyncColorAdjustmentsRequest {
            source_project_id: source.id.clone(),
            project_ids: vec![unrelated.id.clone()],
        })
        .unwrap();
    assert_eq!(result.synced, 0);
    assert_eq!(result.failed, vec![unrelated.name.clone()]);

    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: unrelated.id.clone(),
        })
        .unwrap();
    assert!(reopened.layers.iter().all(|layer| layer.kind != "color"));
}

#[test]
fn syncing_colour_adjustments_copies_lut_files_into_the_target_project() {
    let app = TestApp::new();
    let source_path = app.sample_path();
    let source = app
        .service
        .import_photo(ImportPhotoRequest {
            path: source_path.to_string_lossy().to_string(),
            collection_id: None,
            collection_name: Some("LUT 照片集".to_string()),
        })
        .unwrap();
    let target_path = app.dir.path().join("lut-target.png");
    write_sample(&target_path);
    let target = app
        .service
        .import_photo(ImportPhotoRequest {
            path: target_path.to_string_lossy().to_string(),
            collection_id: Some(source.collection_id.clone()),
            collection_name: None,
        })
        .unwrap();

    let cube_path = app.dir.path().join("warm.cube");
    fs::write(
        &cube_path,
        "TITLE \"warm\"\nLUT_1D_SIZE 3\n0 0 0\n0.9 0.5 0.2\n1 1 1\n",
    )
    .unwrap();
    let lut = app
        .service
        .import_lut(ImportLutRequest {
            project_id: source.id.clone(),
            path: Some(cube_path.to_string_lossy().to_string()),
            file_name: None,
            data_base64: None,
        })
        .unwrap();
    let source_lut_id = Uuid::parse_str(&lut.asset_id).unwrap();

    let source = app
        .service
        .add_layer(AddLayerRequest {
            project_id: source.id.clone(),
            kind: AddLayerKind::Color,
            name: None,
            portrait: None,
            mask: None,
        })
        .unwrap();
    app.service
        .update_layer_params(UpdateLayerParamsRequest {
            project_id: source.id.clone(),
            layer_id: source.layers.last().unwrap().id.clone(),
            color: Some(ColorParams {
                lut_asset_id: Some(source_lut_id),
                lut_strength: 0.6,
                ..Default::default()
            }),
            portrait: None,
            retouch: None,
        })
        .unwrap();
    app.service
        .commit_changes(CommitChangesRequest {
            project_id: source.id.clone(),
            label: "LUT 风格".into(),
        })
        .unwrap();

    let result = app
        .service
        .sync_color_adjustments(SyncColorAdjustmentsRequest {
            source_project_id: source.id.clone(),
            project_ids: vec![target.id.clone()],
        })
        .unwrap();
    assert_eq!(result.synced, 1);

    let reopened = app
        .service
        .open_project(OpenProjectRequest {
            project_id: target.id.clone(),
        })
        .unwrap();
    let color = reopened
        .layers
        .iter()
        .find(|layer| layer.kind == "color")
        .and_then(|layer| layer.color)
        .unwrap();
    let copied_lut = color.lut_asset_id.expect("the look follows the adjustment");
    assert_ne!(copied_lut, source_lut_id, "LUT assets are project-scoped");

    // The target must keep rendering after the source (and its LUT file) is gone.
    app.service
        .delete_project(app_core::ProjectIdRequest {
            project_id: source.id.clone(),
        })
        .unwrap();
    let preview = app
        .service
        .render_preview(RenderPreviewRequest {
            project_id: target.id.clone(),
            max_dimension: Some(32),
            ignore_crop: false,
        })
        .unwrap();
    assert_eq!(preview.backend, "cpu");
}

/// 2x2 opaque white PNG, used to verify watermark compositing without a fixture.
const WHITE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAADklEQVR42mP4DwUMMAYAj4IP8SHNGCcAAAAASUVORK5CYII=";

#[test]
fn exporting_with_a_watermark_composites_it_into_the_corner() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("watermarked.png");
    let result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: Some(WatermarkOptions {
                    image_base64: format!("data:image/png;base64,{WHITE_PNG_BASE64}"),
                    opacity: 1.0,
                    scale: 0.5,
                    margin: 0.0,
                    position: WatermarkPosition::BottomRight,
                }),
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!((result.width, result.height), (64, 48));
    let decoded = image_core::decode_image_from_path(&out).unwrap();
    assert_eq!(decoded.pixel(63, 47), Some([255, 255, 255, 255]));
    assert_eq!(
        decoded.pixel(0, 0),
        Some([0, 0, 0, 255]),
        "the untouched corner keeps the source pixel"
    );
}

#[test]
fn invalid_watermark_payloads_are_rejected_before_encoding() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("bad-watermark.png");
    let error = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: Some(WatermarkOptions {
                    image_base64: "not-an-image".to_string(),
                    opacity: 1.0,
                    scale: 0.2,
                    margin: 0.04,
                    position: WatermarkPosition::Center,
                }),
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert!(
        !out.exists(),
        "a rejected watermark never writes a partial file"
    );
}

#[test]
fn watermarks_are_composited_in_sixteen_bit_exports() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("watermarked-16.png");
    app.service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: Some(WatermarkOptions {
                    image_base64: WHITE_PNG_BASE64.to_string(),
                    opacity: 1.0,
                    scale: 0.5,
                    margin: 0.0,
                    position: WatermarkPosition::BottomRight,
                }),
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Png,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::None,
                bit_depth: ExportBitDepth::Sixteen,
            },
        })
        .unwrap();
    let decoded = image_core::decode_image_from_path(&out).unwrap();
    let corner = decoded.pixel(63, 47).unwrap();
    assert!(
        corner[0] >= 250 && corner[1] >= 250 && corner[2] >= 250,
        "the 16-bit pipeline keeps the watermark white: {corner:?}"
    );
}

#[test]
fn webp_export_is_lossless_and_never_claims_metadata() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("luma.webp");
    let result = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: ExportOptions {
                watermark: None,
                path: out.to_string_lossy().to_string(),
                format: ExportImageFormat::Webp,
                quality: 100,
                max_dimension: None,
                width: None,
                height: None,
                color_space: ExportColorSpace::Srgb,
                metadata: ExportMetadata::Keep,
                bit_depth: ExportBitDepth::Eight,
            },
        })
        .unwrap();
    assert_eq!((result.width, result.height), (64, 48));
    assert!(!result.metadata_written);
    let decoded = image_core::decode_image_from_path(&out).unwrap();
    let source = image_core::decode_image_from_path(&app.sample_path()).unwrap();
    assert_eq!(decoded.rgba8, source.rgba8, "VP8L is lossless");
}

#[test]
fn webp_rejects_sixteen_bit_and_wide_gamut_requests() {
    let app = TestApp::new();
    let state = import(&app);
    let out = app.dir.path().join("luma.webp");
    let mut options = ExportOptions {
        watermark: None,
        path: out.to_string_lossy().to_string(),
        format: ExportImageFormat::Webp,
        quality: 100,
        max_dimension: None,
        width: None,
        height: None,
        color_space: ExportColorSpace::Srgb,
        metadata: ExportMetadata::None,
        bit_depth: ExportBitDepth::Sixteen,
    };
    let error = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options: options.clone(),
        })
        .unwrap_err();
    assert_eq!(error.code, "BIT_DEPTH_UNSUPPORTED");

    options.bit_depth = ExportBitDepth::Eight;
    options.color_space = ExportColorSpace::DisplayP3;
    let error = app
        .service
        .export_image(ExportImageRequest {
            project_id: state.id.clone(),
            job_id: None,
            options,
        })
        .unwrap_err();
    assert_eq!(error.code, "COLOR_SPACE_UNSUPPORTED");
}

#[test]
fn empty_photo_sets_stay_listed_without_counting_as_photos() {
    let app = TestApp::new();
    let empty = app
        .service
        .create_project(CreateProjectRequest {
            name: "空照片集".into(),
        })
        .unwrap();
    let collections = app.service.list_collections().unwrap();
    let set = collections
        .iter()
        .find(|collection| collection.id == empty.collection_id)
        .unwrap();
    assert_eq!(set.photo_count, 0);
    assert_eq!(set.latest_project_id, empty.id);
    assert!(set.cover_thumbnail_path.is_none());

    let photo = app
        .service
        .import_photo(ImportPhotoRequest {
            path: app.sample_path().to_string_lossy().to_string(),
            collection_id: Some(empty.collection_id.clone()),
            collection_name: None,
        })
        .unwrap();
    let collections = app.service.list_collections().unwrap();
    let set = collections
        .iter()
        .find(|collection| collection.id == empty.collection_id)
        .unwrap();
    assert_eq!(set.photo_count, 1);
    assert_eq!(
        set.latest_project_id, photo.id,
        "opening the set should pick the newest real photo"
    );
    assert!(set.cover_thumbnail_path.is_some());
}
