use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};

use image_core::DecodedImage;
use tract_onnx::prelude::*;

use crate::analysis::FaceBox;

const CENTERFACE_BYTES: &[u8] = include_bytes!("../assets/centerface.onnx");
const YUNET_BYTES: &[u8] = include_bytes!("../assets/yunet_2026may.onnx");
/// Both models are fully convolutional and were trained around VGA resolution.
/// Running them at 1600px costs ~8x more CPU without improving stage photos.
const FACE_DETECT_MAX: u32 = 800;
const CENTERFACE_THRESHOLD: f32 = 0.5;
const YUNET_THRESHOLD: f32 = 0.5;
/// A face seen by only one model is accepted only when that model is clearly
/// confident; dual-model agreement keeps lower-scoring faces.
const SINGLE_MODEL_MIN_SCORE: f32 = 0.7;
const MERGE_IOU: f32 = 0.45;

type TypedPlan = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;
type PlanKey = (ModelKind, u32, u32);
type SharedPlan = Arc<Mutex<TypedPlan>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ModelKind {
    CenterFace,
    Yunet,
}

impl ModelKind {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::CenterFace => CENTERFACE_BYTES,
            Self::Yunet => YUNET_BYTES,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CenterFace => "centerface",
            Self::Yunet => "yunet",
        }
    }
}

struct PlanCache {
    plans: Mutex<HashMap<PlanKey, SharedPlan>>,
}

static PLANS: OnceLock<PlanCache> = OnceLock::new();

fn plan_for(kind: ModelKind, width: u32, height: u32) -> Result<SharedPlan, String> {
    let cache = PLANS.get_or_init(|| PlanCache {
        plans: Mutex::new(HashMap::new()),
    });
    let mut plans = cache
        .plans
        .lock()
        .map_err(|_| "face detector plan cache is poisoned".to_string())?;
    if let Some(plan) = plans.get(&(kind, width, height)) {
        return Ok(plan.clone());
    }
    let mut cursor = Cursor::new(kind.bytes());
    let model = tract_onnx::onnx()
        .model_for_read(&mut cursor)
        .map_err(|error| format!("{} model could not be loaded: {error}", kind.label()))?;
    let fact = InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 3, height as i64, width as i64));
    let plan = model
        .with_input_fact(0, fact)
        .map_err(|error| format!("{} input shape failed: {error}", kind.label()))?
        .into_optimized()
        .map_err(|error| format!("{} model optimisation failed: {error}", kind.label()))?
        .into_runnable()
        .map_err(|error| format!("{} model is not runnable: {error}", kind.label()))?;
    let plan = Arc::new(Mutex::new(plan));
    if plans.len() >= 6 {
        plans.clear();
    }
    plans.insert((kind, width, height), plan.clone());
    Ok(plan)
}

fn run_plan(
    plan: &SharedPlan,
    input: &[f32],
    width: u32,
    height: u32,
) -> Result<TVec<TValue>, String> {
    let tensor = Tensor::from_shape(&[1, 3, height as usize, width as usize], input)
        .map_err(|error| error.to_string())?;
    plan.lock()
        .map_err(|_| "face detector plan is poisoned".to_string())?
        .run(tvec!(tensor.into()))
        .map_err(|error| error.to_string())
}

#[derive(Clone, Copy, Debug)]
struct RawDetection {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    score: f32,
    sources: u8,
}

fn iou(a: &RawDetection, b: &RawDetection) -> f32 {
    let x1 = a.x1.max(b.x1);
    let y1 = a.y1.max(b.y1);
    let x2 = a.x2.min(b.x2);
    let y2 = a.y2.min(b.y2);
    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = (a.x2 - a.x1).max(0.0) * (a.y2 - a.y1).max(0.0);
    let area_b = (b.x2 - b.x1).max(0.0) * (b.y2 - b.y1).max(0.0);
    intersection / (area_a + area_b - intersection).max(1e-6)
}

fn nms(mut detections: Vec<RawDetection>, threshold: f32) -> Vec<RawDetection> {
    detections.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept = Vec::new();
    while let Some(best) = detections.first().copied() {
        detections.retain(|detection| iou(&best, detection) < threshold);
        kept.push(best);
    }
    kept
}

fn centerface(image: &DecodedImage) -> Result<Vec<RawDetection>, String> {
    let width = (image.width.div_ceil(32) * 32).max(32);
    let height = (image.height.div_ceil(32) * 32).max(32);
    let resized = image
        .resize_exact(width, height)
        .map_err(|error| error.to_string())?;
    let plane = (width * height) as usize;
    let mut input = vec![0f32; plane * 3];
    for (index, pixel) in resized.rgba8.chunks_exact(4).enumerate() {
        input[index] = pixel[0] as f32;
        input[plane + index] = pixel[1] as f32;
        input[2 * plane + index] = pixel[2] as f32;
    }
    let plan = plan_for(ModelKind::CenterFace, width, height)?;
    let outputs = run_plan(&plan, &input, width, height)?;
    if outputs.len() < 4 {
        return Err(format!(
            "centerface returned {} outputs, expected 4",
            outputs.len()
        ));
    }
    let heatmap = outputs[0]
        .as_slice::<f32>()
        .map_err(|error| error.to_string())?;
    let scale = outputs[1]
        .as_slice::<f32>()
        .map_err(|error| error.to_string())?;
    let offset = outputs[2]
        .as_slice::<f32>()
        .map_err(|error| error.to_string())?;
    let shape = outputs[0].shape();
    if shape.len() != 4 {
        return Err("centerface heatmap has an unexpected shape".into());
    }
    let rows = shape[2] as usize;
    let cols = shape[3] as usize;
    let plane = rows * cols;
    let scale_x = image.width as f32 / width as f32;
    let scale_y = image.height as f32 / height as f32;
    let mut detections = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let index = row * cols + col;
            let score = heatmap[index];
            if score < CENTERFACE_THRESHOLD {
                continue;
            }
            let s0 = scale[plane + index].exp() * 4.0;
            let s1 = scale[index].exp() * 4.0;
            let o0 = offset[plane + index];
            let o1 = offset[index];
            let x1 = (col as f32 + o1 + 0.5) * 4.0 - s1 / 2.0;
            let y1 = (row as f32 + o0 + 0.5) * 4.0 - s0 / 2.0;
            detections.push(RawDetection {
                sources: 1,
                x1: (x1 * scale_x).clamp(0.0, image.width as f32),
                y1: (y1 * scale_y).clamp(0.0, image.height as f32),
                x2: ((x1 + s1) * scale_x).clamp(0.0, image.width as f32),
                y2: ((y1 + s0) * scale_y).clamp(0.0, image.height as f32),
                score: score.clamp(0.0, 1.0),
            });
        }
    }
    Ok(nms(detections, 0.3))
}

fn yunet(image: &DecodedImage) -> Result<Vec<RawDetection>, String> {
    let width = (image.width.div_ceil(32) * 32).max(32);
    let height = (image.height.div_ceil(32) * 32).max(32);
    let plane = (width * height) as usize;
    let mut input = vec![0f32; plane * 3];
    for y in 0..image.height {
        for x in 0..image.width {
            let source = ((y * image.width + x) * 4) as usize;
            let target = (y * width + x) as usize;
            input[target] = image.rgba8[source + 2] as f32;
            input[plane + target] = image.rgba8[source + 1] as f32;
            input[2 * plane + target] = image.rgba8[source] as f32;
        }
    }
    let plan = plan_for(ModelKind::Yunet, width, height)?;
    let outputs = run_plan(&plan, &input, width, height)?;
    if outputs.len() < 12 {
        return Err(format!(
            "yunet returned {} outputs, expected 12",
            outputs.len()
        ));
    }
    let mut detections = Vec::new();
    for (level, stride) in [8usize, 16, 32].into_iter().enumerate() {
        let cols = width as usize / stride;
        let rows = height as usize / stride;
        let cls = outputs[level]
            .as_slice::<f32>()
            .map_err(|error| error.to_string())?;
        let obj = outputs[level + 3]
            .as_slice::<f32>()
            .map_err(|error| error.to_string())?;
        let bbox = outputs[level + 6]
            .as_slice::<f32>()
            .map_err(|error| error.to_string())?;
        for row in 0..rows {
            for col in 0..cols {
                let index = row * cols + col;
                let score = (cls[index].clamp(0.0, 1.0) * obj[index].clamp(0.0, 1.0)).sqrt();
                if score < YUNET_THRESHOLD {
                    continue;
                }
                let base = index * 4;
                let center_x = (col as f32 + bbox[base]) * stride as f32;
                let center_y = (row as f32 + bbox[base + 1]) * stride as f32;
                let box_width = bbox[base + 2].exp() * stride as f32;
                let box_height = bbox[base + 3].exp() * stride as f32;
                detections.push(RawDetection {
                    sources: 2,
                    x1: (center_x - box_width / 2.0).clamp(0.0, image.width as f32),
                    y1: (center_y - box_height / 2.0).clamp(0.0, image.height as f32),
                    x2: (center_x + box_width / 2.0).clamp(0.0, image.width as f32),
                    y2: (center_y + box_height / 2.0).clamp(0.0, image.height as f32),
                    score,
                });
            }
        }
    }
    Ok(nms(detections, 0.3))
}

fn merge(center: Vec<RawDetection>, yunet: Vec<RawDetection>) -> Vec<RawDetection> {
    let mut merged = Vec::new();
    let mut used = vec![false; yunet.len()];
    for detection in center {
        let mut best: Option<(usize, f32)> = None;
        for (index, candidate) in yunet.iter().enumerate() {
            if used[index] {
                continue;
            }
            let overlap = iou(&detection, candidate);
            if overlap >= MERGE_IOU && best.is_none_or(|(_, score)| overlap > score) {
                best = Some((index, overlap));
            }
        }
        if let Some((index, _)) = best {
            used[index] = true;
            let other = yunet[index];
            let boxed = if detection.score >= other.score {
                detection
            } else {
                other
            };
            merged.push(RawDetection {
                score: (detection.score + other.score) * 0.5,
                sources: detection.sources | other.sources,
                ..boxed
            });
        } else if detection.score >= SINGLE_MODEL_MIN_SCORE {
            merged.push(RawDetection {
                score: detection.score * 0.95,
                ..detection
            });
        }
    }
    for (index, detection) in yunet.into_iter().enumerate() {
        if !used[index] && detection.score >= SINGLE_MODEL_MIN_SCORE {
            merged.push(RawDetection {
                score: detection.score * 0.95,
                ..detection
            });
        }
    }
    merged.sort_by(|a, b| {
        a.x1.total_cmp(&b.x1)
            .then_with(|| a.y1.total_cmp(&b.y1))
            .then_with(|| b.score.total_cmp(&a.score))
    });
    merged
}

/// Run both detectors on an upright image and merge their boxes.
pub(crate) fn detect(image: &DecodedImage) -> Result<Vec<FaceBox>, String> {
    let resized;
    let detection = if image.width.max(image.height) > FACE_DETECT_MAX {
        resized = image
            .resize_to_fit(FACE_DETECT_MAX)
            .map_err(|error| error.to_string())?;
        &resized
    } else {
        image
    };
    let scale_x = image.width as f32 / detection.width.max(1) as f32;
    let scale_y = image.height as f32 / detection.height.max(1) as f32;
    let center = centerface(detection)?;
    let yunet = yunet(detection)?;
    let merged = merge(center, yunet);
    Ok(merged
        .into_iter()
        .enumerate()
        .map(|(index, detection)| {
            let mut models = Vec::new();
            if detection.sources & 1 != 0 {
                models.push(ModelKind::CenterFace.label().to_string());
            }
            if detection.sources & 2 != 0 {
                models.push(ModelKind::Yunet.label().to_string());
            }
            FaceBox {
                face_id: format!("face-{}", index + 1),
                x: detection.x1 * scale_x,
                y: detection.y1 * scale_y,
                width: ((detection.x2 - detection.x1) * scale_x).max(1.0),
                height: ((detection.y2 - detection.y1) * scale_y).max(1.0),
                confidence: detection.score,
                models,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nms_keeps_the_strongest_overlapping_box() {
        let detections = vec![
            RawDetection {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 10.0,
                score: 0.8,
                sources: 1,
            },
            RawDetection {
                x1: 1.0,
                y1: 1.0,
                x2: 11.0,
                y2: 11.0,
                score: 0.9,
                sources: 1,
            },
            RawDetection {
                x1: 100.0,
                y1: 100.0,
                x2: 110.0,
                y2: 110.0,
                score: 0.5,
                sources: 1,
            },
        ];
        let kept = nms(detections, 0.3);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.9);
    }

    #[test]
    fn dual_model_agreement_merges_one_box() {
        let center = vec![RawDetection {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
            score: 0.8,
            sources: 1,
        }];
        let yunet = vec![RawDetection {
            x1: 1.0,
            y1: 1.0,
            x2: 11.0,
            y2: 11.0,
            score: 0.9,
            sources: 1,
        }];
        let merged = merge(center, yunet);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].score - 0.85).abs() < 1e-6);
    }

    #[test]
    fn single_model_face_needs_high_confidence() {
        let center = vec![RawDetection {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
            score: 0.6,
            sources: 1,
        }];
        let yunet = vec![RawDetection {
            x1: 100.0,
            y1: 100.0,
            x2: 110.0,
            y2: 110.0,
            score: 0.8,
            sources: 1,
        }];
        let merged = merge(center, yunet);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].score - 0.76).abs() < 1e-6);
    }
}
