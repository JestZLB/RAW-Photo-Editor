use std::path::PathBuf;
use std::sync::Arc;

use ai_provider::{
    now_rfc3339, AiJobOperation, AiJobRequest, AiJobStatus, ImageEditProvider, MockProvider,
    OpenAiCompatibleProvider, OpenAiProviderConfig, ProgressCallback, ProviderError,
    ProviderErrorCode, ProviderInfo, ProviderKind, SecretString,
};
use document::Layer;
use image_core::{decode_image_from_path, encode_atomic, OutputFormat};
use persistence::{AiJobRecord, AssetRecord, Database, ProviderRecord};
use task_runtime::{TaskPriority, TaskRegistry};
use uuid::Uuid;

use super::{job_info, parse_uuid, AppService, EventSink, SOURCE_PREVIEW_MAX};
use crate::dto::DocumentState;
use crate::error::ApiError;
use crate::requests::{
    ApplyAiResultRequest, CancelAiJobRequest, ListAiJobsRequest, ProviderIdRequest,
    SubmitAiJobRequest, SubmitAiJobResponse,
};
use crate::AiJobInfo;

impl AppService {
    pub async fn submit_ai_job(
        &self,
        request: SubmitAiJobRequest,
    ) -> Result<SubmitAiJobResponse, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        if request.operation != AiJobOperation::Edit {
            return Err(ApiError::new(
                "UNSUPPORTED_OPERATION",
                "suggest operations require a text model that is not configured",
                "当前版本暂不支持 AI 提示词建议，请直接输入修图描述。",
            ));
        }
        let prompt = request.prompt.trim();
        if prompt.is_empty() {
            return Err(ApiError::invalid_argument(
                "prompt is empty",
                "请输入 AI 修图描述。",
            ));
        }
        if prompt.chars().count() > 4000 {
            return Err(ApiError::invalid_argument(
                "prompt is too long",
                "AI 描述过长（最多 4000 字）。",
            ));
        }
        if !request.strength.is_finite() {
            return Err(ApiError::invalid_argument(
                "strength is not finite",
                "AI 强度参数无效。",
            ));
        }
        let strength = request.strength.clamp(0.0, 100.0);
        let provider_id = request.provider_id.trim().to_string();
        if provider_id.is_empty() {
            return Err(ApiError::invalid_argument(
                "provider id is empty",
                "请选择 AI Provider。",
            ));
        }

        self.ensure_session(project_id)?;
        let source_document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        let Some(source) = &source_document.source else {
            return Err(ApiError::invalid_argument(
                "project has no source image",
                "请先导入照片，再使用 AI 修图。",
            ));
        };
        let original_path = PathBuf::from(&source.original_path);
        // Providers cannot ingest camera RAW. Use the generated sRGB preview
        // instead; the document still keeps the untouched original RAW path.
        let input_path = if raw_develop::is_raw_path(&original_path) {
            self.assets
                .source_preview_path(project_id, source.asset_id, SOURCE_PREVIEW_MAX)
        } else {
            original_path
        };
        if !input_path.exists() {
            return Err(ApiError::source_missing());
        }

        let provider_record = self.provider_record(&provider_id)?;
        let provider = self.build_provider(&provider_record).await?;
        let info: ProviderInfo = provider.info();
        if let Some(max_bytes) = info.capabilities.max_input_bytes {
            let bytes = std::fs::metadata(&input_path)
                .map_err(|error| ApiError::new("IO", error.to_string(), "无法读取源图片文件。"))?
                .len();
            if bytes > max_bytes {
                return Err(ApiError::from(
                    ProviderError::new(
                        ProviderErrorCode::InputTooLarge,
                        format!("input image is {bytes} bytes, max {max_bytes}"),
                    )
                    .with_user_message("照片文件过大，当前 Provider 无法上传，请压缩后重试。"),
                ));
            }
        }

        self.assets.ensure_project_dirs(project_id)?;
        let job_id = Uuid::new_v4();
        let created_at = now_rfc3339();
        let output_path = self.assets.ai_result_path(project_id, job_id);
        let record = AiJobRecord {
            id: job_id,
            project_id,
            provider_id: provider_record.id.clone(),
            provider_name: provider_record.name.clone(),
            operation: AiJobOperation::Edit.as_str().to_string(),
            prompt: prompt.to_string(),
            status: AiJobStatus::Queued.as_str().to_string(),
            progress: Some(0.0),
            message: Some("排队中".to_string()),
            result_asset_id: None,
            result_path: None,
            error_code: None,
            user_message: None,
            created_at: created_at.clone(),
            updated_at: created_at,
        };
        self.db.upsert_ai_job(&record)?;
        emit_job(&self.db, self.events.as_ref(), job_id);

        let handle = self
            .tasks
            .register(job_id.to_string(), TaskPriority::Background);
        let job = AiJobRequest {
            job_id,
            project_id,
            provider_id: provider_record.id.clone(),
            operation: AiJobOperation::Edit,
            prompt: prompt.to_string(),
            strength,
            input_path,
            output_path,
        };
        let db = self.db.clone();
        let events = self.events.clone();
        let tasks = self.tasks.clone();
        let token = handle.token.clone();
        tokio::spawn(async move {
            run_ai_job(db, events, tasks, provider, job, token).await;
        });

        Ok(SubmitAiJobResponse {
            job_id: job_id.to_string(),
        })
    }

    pub async fn list_provider_models(
        &self,
        request: ProviderIdRequest,
    ) -> Result<Vec<String>, ApiError> {
        let provider_id = request.provider_id.trim();
        if provider_id.is_empty() {
            return Err(ApiError::invalid_argument(
                "provider id is empty",
                "请选择一个 AI 服务。",
            ));
        }
        let record = self.provider_record(provider_id)?;
        let provider = self.build_provider(&record).await?;
        provider.list_models().await.map_err(ApiError::from)
    }

    pub fn cancel_ai_job(&self, request: CancelAiJobRequest) -> Result<(), ApiError> {
        let job_id = parse_uuid(&request.job_id, "job")?;
        let Some(record) = self.db.get_ai_job(job_id)? else {
            return Err(job_not_found());
        };
        if AiJobStatus::from_label(&record.status).is_terminal() {
            return Ok(());
        }
        self.tasks.cancel(&job_id.to_string());
        let changed = self
            .db
            .cancel_ai_job_if_active(job_id, "任务已取消。", &now_rfc3339())?;
        if changed {
            emit_job(&self.db, self.events.as_ref(), job_id);
        }
        Ok(())
    }

    pub fn list_ai_jobs(&self, request: ListAiJobsRequest) -> Result<Vec<AiJobInfo>, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        if self.db.get_project(project_id)?.is_none() {
            return Err(ApiError::project_not_found());
        }
        let records = self.db.list_ai_jobs(project_id)?;
        Ok(records.iter().map(job_info).collect())
    }

    pub fn apply_ai_result(
        &self,
        request: ApplyAiResultRequest,
    ) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let job_id = parse_uuid(&request.job_id, "job")?;
        let job = self.db.get_ai_job(job_id)?.ok_or_else(job_not_found)?;
        if job.project_id != project_id {
            return Err(ApiError::invalid_argument(
                "job belongs to another project",
                "该 AI 任务不属于当前项目。",
            ));
        }
        if AiJobStatus::from_label(&job.status) != AiJobStatus::Completed {
            return Err(ApiError::invalid_argument(
                "AI job is not completed",
                "AI 结果尚未生成完成，无法应用。",
            ));
        }
        let asset_id = job.result_asset_id.ok_or_else(|| {
            ApiError::new(
                "ASSET_MISSING",
                "completed AI job has no result asset",
                "AI 结果图片缺失，请重新生成。",
            )
        })?;
        let asset = self.db.get_asset(asset_id)?.ok_or_else(|| {
            ApiError::new(
                "ASSET_MISSING",
                "AI result asset row is missing",
                "AI 结果图片缺失，请重新生成。",
            )
        })?;
        if asset.project_id != project_id || asset.kind != "ai_result" {
            return Err(ApiError::new(
                "ASSET_MISSING",
                "AI result asset does not belong to this project",
                "AI 结果图片缺失，请重新生成。",
            ));
        }
        let asset_path = PathBuf::from(&asset.path);
        if !asset_path.exists() {
            return Err(ApiError::new(
                "ASSET_MISSING",
                "AI result file is missing on disk",
                "AI 结果图片缺失，请重新生成。",
            ));
        }
        // Validate the result and refresh its preview before modelling a layer.
        let decoded = decode_image_from_path(&asset_path)?;
        self.assets.ensure_project_dirs(project_id)?;
        let preview = decoded.resize_to_fit(SOURCE_PREVIEW_MAX)?;
        let _ = encode_atomic(
            &self
                .assets
                .ai_preview_path(project_id, asset_id, SOURCE_PREVIEW_MAX),
            &preview,
            OutputFormat::Png,
            100,
        );

        self.session(project_id, |project| {
            if project.session.document().layers.iter().any(|layer| {
                layer.kind == document::LayerKind::AiResult && layer.asset_id == Some(asset_id)
            }) {
                return Err(ApiError::invalid_argument(
                    "AI result already applied",
                    "该 AI 结果已经应用到项目。",
                ));
            }
            let mut layer = Layer::ai_result(asset_id);
            let prompt = job.prompt.trim();
            let short: String = prompt.chars().take(18).collect();
            if !short.is_empty() {
                layer.name = if prompt.chars().count() > 18 {
                    format!("AI · {short}…")
                } else {
                    format!("AI · {short}")
                };
            }
            project
                .session
                .try_mutate(|document| document.add_layer(layer).map_err(ApiError::from))?;
            project.session.commit("应用 AI 效果");
            project.request_save();
            Ok(self.document_state(project_id, &project.session))
        })
    }

    async fn build_provider(
        &self,
        record: &ProviderRecord,
    ) -> Result<Arc<dyn ImageEditProvider>, ApiError> {
        let kind = ProviderKind::from_label(&record.kind)
            .ok_or_else(|| ApiError::internal("unknown provider kind in database"))?;
        match kind {
            ProviderKind::Mock => {
                let provider = MockProvider::with_identity(
                    record.id.clone(),
                    record.name.clone(),
                    record.created_at.clone(),
                );
                provider.validate().await?;
                Ok(Arc::new(provider))
            }
            ProviderKind::OpenAiCompatible => {
                let base_url = record
                    .base_url
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        ApiError::invalid_argument(
                            "openai provider has no base url",
                            "请先在设置中填写 OpenAI 兼容服务地址。",
                        )
                    })?;
                let credential = self
                    .credentials
                    .get(&record.id)?
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        ApiError::new(
                            "AUTH_FAILED",
                            "provider has no stored credential",
                            "尚未设置 API Key，请先在设置中保存凭据。",
                        )
                    })?;
                let config = OpenAiProviderConfig {
                    id: record.id.clone(),
                    name: record.name.clone(),
                    base_url,
                    model: record.model.clone(),
                    allow_localhost: record.allow_localhost,
                    created_at: record.created_at.clone(),
                    credential: SecretString::new(credential),
                };
                let provider = OpenAiCompatibleProvider::new(config)?;
                provider.validate().await?;
                Ok(Arc::new(provider))
            }
        }
    }
}

async fn run_ai_job(
    db: Arc<Database>,
    events: Arc<dyn EventSink>,
    tasks: TaskRegistry,
    provider: Arc<dyn ImageEditProvider>,
    job: AiJobRequest,
    token: task_runtime::CancellationToken,
) {
    let job_id = job.job_id;
    let task_id = job_id.to_string();
    let started_at = now_rfc3339();
    let started = match db.mark_ai_job_running(job_id, &started_at) {
        Ok(true) => true,
        Ok(false) => false,
        Err(_) => false,
    };
    if !started {
        tasks.finish(&task_id);
        return;
    }
    emit_job(&db, events.as_ref(), job_id);

    let progress_db = db.clone();
    let progress_events = events.clone();
    let progress: ProgressCallback = Arc::new(move |value: u8| {
        let value = value.min(99) as f32;
        if progress_db
            .update_ai_job_progress(job_id, value, Some("正在生成"), &now_rfc3339())
            .unwrap_or(false)
        {
            emit_job(&progress_db, progress_events.as_ref(), job_id);
        }
    });

    let result = provider
        .execute(
            &job.input_path,
            &job.prompt,
            job.strength,
            &job.output_path,
            progress,
            token.clone(),
        )
        .await;

    match result {
        Ok(_output) => {
            let path = job.output_path.clone();
            let joined = tokio::task::spawn_blocking(move || decode_image_from_path(&path)).await;
            let decoded = match joined {
                Ok(Ok(decoded)) => decoded,
                Ok(Err(_)) | Err(_) => {
                    remove_partial(&job.output_path);
                    fail_job(
                        &db,
                        events.as_ref(),
                        job_id,
                        "INVALID_RESPONSE",
                        "AI 服务返回的图片无法读取，请重试。",
                    );
                    tasks.finish(&task_id);
                    return;
                }
            };
            let bytes = match std::fs::metadata(&job.output_path) {
                Ok(metadata) => metadata.len(),
                Err(_) => {
                    remove_partial(&job.output_path);
                    fail_job(
                        &db,
                        events.as_ref(),
                        job_id,
                        "IO",
                        "AI 结果无法保存，请检查磁盘空间。",
                    );
                    tasks.finish(&task_id);
                    return;
                }
            };
            let asset_id = Uuid::new_v4();
            let asset = AssetRecord {
                id: asset_id,
                project_id: job.project_id,
                kind: "ai_result".to_string(),
                path: job.output_path.to_string_lossy().to_string(),
                width: decoded.width,
                height: decoded.height,
                bytes,
                created_at: now_rfc3339(),
            };
            if db.insert_asset(&asset).is_err() {
                remove_partial(&job.output_path);
                fail_job(
                    &db,
                    events.as_ref(),
                    job_id,
                    "DATABASE_ERROR",
                    "无法保存 AI 结果，请检查磁盘空间后重试。",
                );
                tasks.finish(&task_id);
                return;
            }
            let completed = db
                .complete_ai_job_if_active(
                    job_id,
                    100.0,
                    Some("生成完成"),
                    asset_id,
                    &asset.path,
                    &now_rfc3339(),
                )
                .unwrap_or(false);
            if completed {
                emit_job(&db, events.as_ref(), job_id);
            } else {
                // The user cancelled while the provider was finishing, so no
                // job will ever reference this asset. Drop both the row and the
                // file instead of leaving an orphan behind.
                let _ = db.delete_asset(asset_id);
                remove_partial(&job.output_path);
            }
        }
        Err(error) => {
            remove_partial(&job.output_path);
            if error.is_cancelled() || token.is_cancelled() {
                let now = now_rfc3339();
                if db
                    .cancel_ai_job_if_active(job_id, "任务已取消。", &now)
                    .unwrap_or(false)
                {
                    emit_job(&db, events.as_ref(), job_id);
                }
            } else {
                let now = now_rfc3339();
                if db
                    .fail_ai_job_if_active(job_id, error.code_str(), &error.user_message, &now)
                    .unwrap_or(false)
                {
                    emit_job(&db, events.as_ref(), job_id);
                }
            }
        }
    }
    tasks.finish(&task_id);
}

fn fail_job(db: &Database, events: &dyn EventSink, job_id: Uuid, code: &str, user_message: &str) {
    if db
        .fail_ai_job_if_active(job_id, code, user_message, &now_rfc3339())
        .unwrap_or(false)
    {
        emit_job(db, events, job_id);
    }
}

fn emit_job(db: &Database, events: &dyn EventSink, job_id: Uuid) {
    if let Ok(Some(record)) = db.get_ai_job(job_id) {
        events.ai_job_updated(&job_info(&record));
    }
}

fn remove_partial(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

fn job_not_found() -> ApiError {
    ApiError::new(
        "JOB_NOT_FOUND",
        "ai job not found",
        "AI 任务不存在或已被删除。",
    )
}
