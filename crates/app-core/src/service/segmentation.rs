//! User-supplied segmentation model wiring.
//!
//! No weights ship with the app. The model path comes from settings, the probe
//! runs one real inference so a wrong contract fails loudly, and the result is
//! written as a raster mask (ADR-014) on the selected layer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image_core::DecodedImage;
use render_engine::RasterCoverage;
use segmentation::{SegmentationError, SegmentationModel};

use super::{parse_uuid, AppService};
use crate::dto::{DocumentState, SegmentationCapability};
use crate::error::ApiError;
use crate::requests::{SegmentMaskRequest, SegmentationCapabilityRequest};

/// Two models is enough for A/B comparisons; each one is tens of MB of weights.
const MAX_CACHED_MODELS: usize = 2;
/// Probe frame value: flat mid-grey, which exercises the contract without
/// depending on image content.
const PROBE_PIXEL: u8 = 128;

fn model_error(error: SegmentationError) -> ApiError {
    ApiError::new(error.code(), error.to_string(), error.user_message())
}

fn unavailable(reason: impl Into<String>) -> SegmentationCapability {
    SegmentationCapability {
        available: false,
        reason: Some(reason.into()),
    }
}

fn from_outcome(outcome: &Result<(), String>) -> SegmentationCapability {
    match outcome {
        Ok(()) => SegmentationCapability {
            available: true,
            reason: None,
        },
        Err(reason) => unavailable(reason.clone()),
    }
}

fn normalized_model_path(raw: &Option<String>) -> Option<PathBuf> {
    let value = raw.as_deref()?.trim();
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

impl AppService {
    pub fn check_segmentation_capability(
        &self,
        request: SegmentationCapabilityRequest,
    ) -> SegmentationCapability {
        let Some(path) = normalized_model_path(&request.model_path) else {
            return unavailable("尚未选择分割模型。请在设置里选择 .onnx 文件。");
        };
        if !path.is_file() {
            return unavailable("分割模型文件不存在，请重新选择。");
        }
        if let Ok(cache) = self.segmentation_probe.lock() {
            if let Some((cached_path, outcome)) = cache.as_ref() {
                if cached_path == &path {
                    return from_outcome(outcome);
                }
            }
        }
        let outcome = self
            .probe_segmentation_model(&path)
            .map_err(|error| error.to_string());
        if let Ok(mut cache) = self.segmentation_probe.lock() {
            *cache = Some((path, outcome.clone()));
        }
        from_outcome(&outcome)
    }

    pub fn run_segmentation_mask(
        &self,
        request: SegmentMaskRequest,
    ) -> Result<DocumentState, ApiError> {
        let project_id = parse_uuid(&request.project_id, "project")?;
        let layer_id = parse_uuid(&request.layer_id, "layer")?;
        let path = normalized_model_path(&Some(request.model_path)).ok_or_else(|| {
            ApiError::invalid_argument(
                "segmentation model path is empty",
                "请先在设置里选择分割模型。",
            )
        })?;
        if !path.is_file() {
            return Err(ApiError::new(
                "SEGMENTATION_MODEL_INVALID",
                format!("model file is missing: {}", path.display()),
                "分割模型文件不存在，请重新选择。",
            ));
        }
        let model = self.segmentation_model(&path)?;
        self.ensure_session(project_id)?;
        let document =
            self.session(project_id, |project| Ok(project.session.document().clone()))?;
        // The 1600px proxy is plenty: the model tiles at 512 and the mask is
        // stored at proxy resolution.
        let image = self.load_analysis_image(project_id, &document)?;
        let coverage = model.segment(&image).map_err(model_error)?;
        let raster = RasterCoverage::from_values(coverage.width, coverage.height, coverage.values)
            .ok_or_else(|| {
                ApiError::new(
                    "SEGMENTATION_FAILED",
                    "segmentation produced an empty mask",
                    "分割模型没有输出有效的蒙版。",
                )
            })?;
        self.apply_raster_mask(
            project_id,
            layer_id,
            &raster,
            "onnx:foreground",
            request.threshold.unwrap_or(0.5),
            request.softness.unwrap_or(0.12),
        )
    }

    fn segmentation_model(&self, path: &Path) -> Result<Arc<SegmentationModel>, ApiError> {
        if let Ok(cache) = self.segmentation_models.lock() {
            if let Some(model) = cache.get(path) {
                return Ok(model.clone());
            }
        }
        let model = Arc::new(SegmentationModel::load(path).map_err(model_error)?);
        if let Ok(mut cache) = self.segmentation_models.lock() {
            if cache.len() >= MAX_CACHED_MODELS {
                cache.clear();
            }
            cache.insert(path.to_path_buf(), model.clone());
        }
        Ok(model)
    }

    /// Load the model and run it once on a flat frame. This is the only way to
    /// prove the input/output contract matches — a model that loads but returns
    /// `[1,3,H,W]` would otherwise produce a garbage mask at edit time.
    fn probe_segmentation_model(&self, path: &Path) -> Result<(), SegmentationError> {
        let model = SegmentationModel::load(path)?;
        let side = segmentation::TILE as u32;
        let probe = DecodedImage::new(side, side, vec![PROBE_PIXEL; (side * side * 4) as usize])
            .map_err(|error| SegmentationError::Run(error.to_string()))?;
        let coverage = model.segment(&probe)?;
        if coverage.width != side
            || coverage.height != side
            || coverage.values.len() != (side * side) as usize
        {
            return Err(SegmentationError::WrongOutput(format!(
                "{}x{} from {} values",
                coverage.width,
                coverage.height,
                coverage.values.len()
            )));
        }
        Ok(())
    }
}
