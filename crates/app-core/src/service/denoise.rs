use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ai_denoise::DenoiseModel;
use image_core::DecodedImage;
use persistence::AssetRecord;
use task_runtime::{CancellationToken, TaskPriority};
use uuid::Uuid;

use super::{parse_uuid, AppService, SOURCE_PREVIEW_MAX};
use crate::dto::{AiDenoiseCapability, AiEngineCapability, DocumentState};
use crate::error::ApiError;
use crate::requests::{
    AiCapabilityRequest, AiDenoiseRequest, CancelAiJobRequest, PhotoHqDenoiseRequest,
};

/// Loaded ONNX models are cached: optimising a graph takes seconds. Keep only
/// one, because a photo model can hold tens to hundreds of MB of weights and
/// users rarely alternate between two models within one session.
const MAX_CACHED_DENOISE_MODELS: usize = 1;

/// Photo-HQ is a 4x model. Feeding the full source of a multi-megapixel photo
/// would allocate an enormous output, so the pass is limited to this longest
/// edge and the 4x result is scaled back to the source size afterwards.
const PHOTO_HQ_INPUT_MAX: u32 = 1600;
const PHOTO_HQ_MODEL: &str = "Photo-HQ-W4xEX";
const PHOTO_HQ_TIMEOUT: Duration = Duration::from_secs(600);

impl AppService {
    /// Run a local ONNX denoiser on the source photo and add the result as a
    /// new AI layer. The original pixels are never modified.
    pub fn run_ai_denoise(&self, request: AiDenoiseRequest) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let model_path = PathBuf::from(request.model_path.trim());
        if !model_path.is_file() {
            return Err(ApiError::invalid_argument(
                "denoise model file is missing",
                "没有找到降噪模型文件，请在设置中重新选择。",
            ));
        }
        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        let source = self.load_source_image(project_id, &document)?;
        let model = self.denoise_model(&model_path)?;
        let strength = request.strength.unwrap_or(1.0).clamp(0.0, 1.0);
        let denoised = model.denoise(&source, strength).map_err(|error| {
            ApiError::new(
                "AI_DENOISE_FAILED",
                error.to_string(),
                "AI 降噪没有完成。请确认模型是 ONNX 格式、输入为 [1,3,H,W] 且输出保持原尺寸。",
            )
        })?;
        self.store_denoise_layer(project_id, &denoised, "AI 降噪", "AI 降噪")
    }

    /// Report whether the local AI denoise paths can actually run on this
    /// machine. The Photo-HQ check runs a tiny real inference once and caches
    /// the result, so a missing Vulkan driver is caught before the user waits
    /// for a full-size pass.
    pub fn ai_denoise_capability(
        &self,
        request: AiCapabilityRequest,
        engine_dir: Option<&Path>,
    ) -> AiDenoiseCapability {
        AiDenoiseCapability {
            photo_hq: self.photo_hq_capability(engine_dir),
            onnx: onnx_capability(request.onnx_model_path.as_deref()),
        }
    }

    fn photo_hq_capability(&self, engine_dir: Option<&Path>) -> AiEngineCapability {
        let unavailable = |reason: &str| AiEngineCapability {
            available: false,
            reason: Some(reason.to_string()),
        };
        if !cfg!(windows) {
            return unavailable("Photo-HQ 本地引擎目前只支持 Windows");
        }
        let Some(engine_dir) = engine_dir else {
            return unavailable("没有找到内置 Photo-HQ 引擎");
        };
        if let Err(error) = validate_photo_hq_engine(engine_dir) {
            return unavailable(&error.user_message);
        }
        let mut cache = match self.photo_hq_probe.lock() {
            Ok(cache) => cache,
            Err(_) => return unavailable("AI 引擎状态不可用，请重启应用"),
        };
        if let Some(result) = cache.as_ref() {
            return match result {
                Ok(()) => AiEngineCapability {
                    available: true,
                    reason: None,
                },
                Err(reason) => unavailable(reason),
            };
        }
        let result = probe_photo_hq_engine(engine_dir);
        *cache = Some(result.clone());
        match result {
            Ok(()) => AiEngineCapability {
                available: true,
                reason: None,
            },
            Err(reason) => unavailable(&reason),
        }
    }

    /// Run the bundled Real-ESRGAN ncnn runtime with the Photo-HQ-W4xEX 4x
    /// model, downscale the result to the source size, and blend by strength.
    ///
    /// A client job id registers a cancellation token; `cancel_ai_denoise`
    /// kills the engine process at the next poll before any layer is stored.
    pub fn run_photo_hq_denoise(
        &self,
        request: PhotoHqDenoiseRequest,
        engine_dir: &Path,
    ) -> Result<DocumentState, ApiError> {
        let task = request
            .job_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .map(|id| self.tasks.register(id, TaskPriority::Background));
        let token = task.as_ref().map(|task| task.token.clone());
        let result = self.run_photo_hq_denoise_tracked(request, engine_dir, token.as_ref());
        if let Some(task) = task {
            self.tasks.finish(&task.id);
        }
        result
    }

    pub fn cancel_ai_denoise(&self, request: CancelAiJobRequest) -> bool {
        let job_id = request.job_id.trim();
        if job_id.is_empty() {
            return false;
        }
        self.tasks.cancel(job_id)
    }

    fn run_photo_hq_denoise_tracked(
        &self,
        request: PhotoHqDenoiseRequest,
        engine_dir: &Path,
        token: Option<&CancellationToken>,
    ) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        validate_photo_hq_engine(engine_dir)?;
        check_photo_hq_cancelled(token)?;
        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        let source = self.load_source_image(project_id, &document)?;
        let package = source.resize_to_fit(PHOTO_HQ_INPUT_MAX)?;
        let workdir = tempfile::Builder::new()
            .prefix("luma-photo-hq-")
            .tempdir()
            .map_err(|error| {
                ApiError::internal(format!("cannot create photo-hq temp dir: {error}"))
            })?;
        let input_path = workdir.path().join("input.png");
        let output_path = workdir.path().join("output.png");
        let log_path = workdir.path().join("engine.log");
        image_core::encode_atomic(&input_path, &package, image_core::OutputFormat::Png, 100)?;
        // The engine only needs the PNG; release the 1600px buffer before the
        // 4x output is decoded.
        drop(package);
        check_photo_hq_cancelled(token)?;
        run_photo_hq_engine(engine_dir, &input_path, &output_path, &log_path, 192, token)?;
        check_photo_hq_cancelled(token)?;
        let model_output = image_core::decode_image_from_path(&output_path).map_err(|error| {
            ApiError::new(
                "AI_DENOISE_FAILED",
                format!("photo-hq output is not a readable image: {error}"),
                "Photo-HQ 的输出文件无法读取，请重试或换一张照片。",
            )
        })?;
        let resized = model_output.resize_exact(source.width, source.height)?;
        drop(model_output);
        let strength = request.strength.unwrap_or(1.0).clamp(0.0, 1.0);
        let denoised = blend_with_source(&source, resized, strength)?;
        self.store_denoise_layer(project_id, &denoised, "AI 降噪 · Photo-HQ", "Photo-HQ 降噪")
    }

    fn denoise_model(&self, path: &Path) -> Result<Arc<DenoiseModel>, ApiError> {
        let mut cache = self
            .denoise_models
            .lock()
            .map_err(|_| ApiError::internal("denoise model cache is poisoned"))?;
        if let Some(model) = cache.get(path) {
            return Ok(model.clone());
        }
        let model = Arc::new(DenoiseModel::load(path).map_err(|error| {
            ApiError::new(
                "AI_DENOISE_MODEL_INVALID",
                error.to_string(),
                "这个降噪模型无法加载，请确认是兼容的 ONNX 模型。",
            )
        })?);
        if cache.len() >= MAX_CACHED_DENOISE_MODELS {
            cache.clear();
        }
        cache.insert(path.to_path_buf(), model.clone());
        Ok(model)
    }

    fn store_denoise_layer(
        &self,
        project_id: Uuid,
        image: &DecodedImage,
        layer_name: &str,
        history_label: &str,
    ) -> Result<DocumentState, ApiError> {
        self.assets.ensure_project_dirs(project_id)?;
        let asset_id = Uuid::new_v4();
        let path = self.assets.ai_result_path(project_id, asset_id);
        let bytes = image_core::encode_atomic(&path, image, image_core::OutputFormat::Png, 100)?;
        self.db.insert_asset(&AssetRecord {
            id: asset_id,
            project_id,
            kind: "ai_result".to_string(),
            path: path.to_string_lossy().to_string(),
            width: image.width,
            height: image.height,
            bytes,
            created_at: super::now_rfc3339(),
        })?;
        if let Ok(preview) = image.resize_to_fit(SOURCE_PREVIEW_MAX) {
            let _ = image_core::encode_atomic(
                &self
                    .assets
                    .ai_preview_path(project_id, asset_id, SOURCE_PREVIEW_MAX),
                &preview,
                image_core::OutputFormat::Png,
                100,
            );
        }

        self.session(project_id, |project| {
            let mut layer = document::Layer::ai_result(asset_id);
            layer.name = layer_name.to_string();
            project
                .session
                .try_mutate(|document| document.add_layer(layer).map_err(ApiError::from))?;
            project.session.commit(history_label);
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }
}

fn check_photo_hq_cancelled(token: Option<&CancellationToken>) -> Result<(), ApiError> {
    if token.is_some_and(CancellationToken::is_cancelled) {
        return Err(photo_hq_cancelled());
    }
    Ok(())
}

fn photo_hq_cancelled() -> ApiError {
    ApiError::new(
        "AI_DENOISE_CANCELLED",
        "photo-hq run was cancelled",
        "已取消 Photo-HQ 降噪，没有生成新图层。",
    )
}

fn validate_photo_hq_engine(engine_dir: &Path) -> Result<(), ApiError> {
    let missing = || {
        ApiError::new(
            "AI_DENOISE_ENGINE_MISSING",
            "photo-hq engine files are missing",
            "Photo-HQ 引擎不完整，请重新安装应用，或把引擎目录放到 LocalAppData 的 engines/realesrgan 下。",
        )
    };
    if !cfg!(windows) {
        return Err(ApiError::new(
            "AI_DENOISE_ENGINE_MISSING",
            "photo-hq engine is Windows-only",
            "Photo-HQ 本地引擎目前只支持 Windows。",
        ));
    }
    if !engine_dir.join("realesrgan-ncnn-vulkan.exe").is_file() {
        return Err(missing());
    }
    let models = engine_dir.join("models");
    if !models.join(format!("{PHOTO_HQ_MODEL}.param")).is_file()
        || !models.join(format!("{PHOTO_HQ_MODEL}.bin")).is_file()
    {
        return Err(missing());
    }
    Ok(())
}

fn onnx_capability(model_path: Option<&str>) -> AiEngineCapability {
    match model_path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) if Path::new(path).is_file() => AiEngineCapability {
            available: true,
            reason: None,
        },
        Some(_) => AiEngineCapability {
            available: false,
            reason: Some("没有找到配置的 ONNX 模型文件".to_string()),
        },
        None => AiEngineCapability {
            available: false,
            reason: Some("尚未选择 ONNX 模型".to_string()),
        },
    }
}

/// Run a 32×32 pass so the probe exercises model loading, Vulkan init and the
/// engine's path handling, not just file existence.
fn probe_photo_hq_engine(engine_dir: &Path) -> Result<(), String> {
    let workdir = tempfile::Builder::new()
        .prefix("luma-photo-hq-probe-")
        .tempdir()
        .map_err(|error| format!("无法创建自检目录：{error}"))?;
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let value = if (x / 4 + y / 4) % 2 == 0 { 230 } else { 25 };
            rgba.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let image = DecodedImage::new(32, 32, rgba).map_err(|error| error.to_string())?;
    let input = workdir.path().join("probe.png");
    let output = workdir.path().join("probe-out.png");
    let log = workdir.path().join("probe.log");
    image_core::encode_atomic(&input, &image, image_core::OutputFormat::Png, 100)
        .map_err(|error| error.to_string())?;
    run_photo_hq_engine(engine_dir, &input, &output, &log, 32, None)
        .map_err(|error| format!("引擎自检失败：{}", error.user_message))?;
    let result = image_core::decode_image_from_path(&output)
        .map_err(|error| format!("引擎自检输出无法读取：{error}"))?;
    if result.width < 64 || result.height < 64 {
        return Err("引擎自检输出尺寸异常".to_string());
    }
    Ok(())
}

fn run_photo_hq_engine(
    engine_dir: &Path,
    input: &Path,
    output: &Path,
    log_path: &Path,
    tile: u32,
    token: Option<&CancellationToken>,
) -> Result<(), ApiError> {
    let mut command = Command::new(engine_dir.join("realesrgan-ncnn-vulkan.exe"));
    command
        .current_dir(engine_dir)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(output)
        .arg("-n")
        .arg(PHOTO_HQ_MODEL)
        // Keep this relative: Tauri's resource path carries a `\\?\` prefix,
        // which this ncnn binary mis-joins with the working directory.
        .arg("-m")
        .arg("models")
        .arg("-s")
        .arg("4")
        .arg("-t")
        .arg(tile.to_string())
        .arg("-f")
        .arg("png");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Do not flash a console window for the 10-60s inference pass.
        command.creation_flags(0x0800_0000);
    }
    let log = File::create(log_path).map_err(|error| {
        ApiError::internal(format!("cannot create photo-hq engine log: {error}"))
    })?;
    let log_err = log.try_clone().map_err(|error| {
        ApiError::internal(format!("cannot duplicate photo-hq engine log: {error}"))
    })?;
    command
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    let mut child = command.spawn().map_err(|error| {
        ApiError::new(
            "AI_DENOISE_FAILED",
            format!("cannot start photo-hq engine: {error}"),
            "Photo-HQ 引擎无法启动，请确认应用文件完整。",
        )
    })?;

    let started = Instant::now();
    let status = loop {
        if token.is_some_and(CancellationToken::is_cancelled) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(photo_hq_cancelled());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                return Err(ApiError::new(
                    "AI_DENOISE_FAILED",
                    format!("photo-hq engine wait failed: {error}"),
                    "Photo-HQ 引擎运行失败，请重试。",
                ));
            }
        }
        if started.elapsed() > PHOTO_HQ_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ApiError::new(
                "AI_DENOISE_TIMEOUT",
                "photo-hq engine timed out",
                "Photo-HQ 降噪超时，请确认显卡驱动正常，或降低分辨率后重试。",
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    };

    if !status.success() {
        let log = std::fs::read_to_string(log_path).unwrap_or_default();
        let tail: Vec<&str> = log.lines().rev().take(6).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
        return Err(ApiError::new(
            "AI_DENOISE_FAILED",
            format!("photo-hq engine exited with {status}: {tail}"),
            "Photo-HQ 降噪没有完成。请确认显卡驱动支持 Vulkan，并且引擎可以独立运行。",
        ));
    }
    if !output.is_file() {
        return Err(ApiError::new(
            "AI_DENOISE_FAILED",
            "photo-hq engine produced no output",
            "Photo-HQ 没有生成结果文件，请重试。",
        ));
    }
    Ok(())
}

/// Blend the model output over the original at `strength`, preserving the
/// source alpha channel. The model buffer is consumed and blended in place so
/// a full-resolution 4-channel copy is not allocated on top of the source.
fn blend_with_source(
    source: &DecodedImage,
    mut model: DecodedImage,
    strength: f32,
) -> Result<DecodedImage, ApiError> {
    if model.width != source.width || model.height != source.height {
        return Err(ApiError::internal(
            "photo-hq blend buffer dimensions do not match the source",
        ));
    }
    let strength = strength.clamp(0.0, 1.0);
    for (original, generated) in source
        .rgba8
        .chunks_exact(4)
        .zip(model.rgba8.chunks_exact_mut(4))
    {
        for channel in 0..3 {
            let value = original[channel] as f32
                + (generated[channel] as f32 - original[channel] as f32) * strength;
            generated[channel] = value.round().clamp(0.0, 255.0) as u8;
        }
        generated[3] = original[3];
    }
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(value: [u8; 4], width: u32, height: u32) -> DecodedImage {
        DecodedImage::new(width, height, value.repeat((width * height) as usize)).unwrap()
    }

    #[test]
    fn onnx_capability_requires_an_existing_model_file() {
        assert!(!onnx_capability(None).available);
        assert!(!onnx_capability(Some("  ")).available);
        assert!(!onnx_capability(Some("missing-model.onnx")).available);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("denoise.onnx");
        std::fs::write(&path, b"not-a-real-model").unwrap();
        let capability = onnx_capability(Some(path.to_str().unwrap()));
        assert!(capability.available);
        assert!(capability.reason.is_none());
    }

    #[test]
    fn photo_hq_blend_respects_strength_and_alpha() {
        let source = solid([10, 20, 30, 128], 4, 3);
        let model = solid([210, 220, 230, 255], 4, 3);
        let none = blend_with_source(&source, model.clone(), 0.0).unwrap();
        assert_eq!(none.rgba8, source.rgba8);
        let full = blend_with_source(&source, model.clone(), 1.0).unwrap();
        assert_eq!(&full.rgba8[..4], &[210, 220, 230, 128]);
        let half = blend_with_source(&source, model, 0.5).unwrap();
        assert_eq!(&half.rgba8[..4], &[110, 120, 130, 128]);
    }
}
