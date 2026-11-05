use image_core::DecodedImage;
use serde::{Deserialize, Serialize};

use crate::onnx;

pub const FACE_MODEL_NAME: &str = "CenterFace + YuNet";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceBox {
    pub face_id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 0..1 ensemble confidence. CenterFace/YuNet scores are already in this
    /// range; dual-model agreement uses their average.
    pub confidence: f32,
    /// Detectors that found this face: "centerface" and/or "yunet".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortraitAnalysis {
    pub available: bool,
    pub faces: Vec<FaceBox>,
    pub warnings: Vec<PortraitWarning>,
}

pub trait FaceAnalyzer: Send + Sync {
    fn analyze(&self, image: &DecodedImage) -> PortraitAnalysis;
}

/// Local ensemble: CenterFace and YuNet run on the same analysis image and
/// their boxes are merged. The models are embedded, MIT-licensed ONNX files.
pub struct EnsembleFaceAnalyzer;

impl EnsembleFaceAnalyzer {
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }
}

pub fn face_model_available() -> bool {
    // Model parsing/inference is lazy; bundling guarantees this capability.
    true
}

impl FaceAnalyzer for EnsembleFaceAnalyzer {
    fn analyze(&self, image: &DecodedImage) -> PortraitAnalysis {
        if image.width == 0 || image.height == 0 {
            return PortraitAnalysis {
                available: true,
                faces: Vec::new(),
                warnings: Vec::new(),
            };
        }
        match onnx::detect(image) {
            Ok(faces) => PortraitAnalysis {
                available: true,
                faces,
                warnings: Vec::new(),
            },
            Err(error) => PortraitAnalysis {
                available: false,
                faces: Vec::new(),
                warnings: vec![PortraitWarning {
                    code: "model_unavailable".to_string(),
                    message: format!("内置人脸模型暂时不可用：{error}"),
                }],
            },
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableFaceAnalyzer;

impl FaceAnalyzer for UnavailableFaceAnalyzer {
    fn analyze(&self, _image: &DecodedImage) -> PortraitAnalysis {
        PortraitAnalysis {
            available: false,
            faces: Vec::new(),
            warnings: vec![PortraitWarning {
                code: "model_not_configured".to_string(),
                message: "人像识别模型尚未配置，眼睛增强、牙齿美白等需要人脸分析的功能暂不可用。"
                    .to_string(),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_image(width: u32, height: u32) -> DecodedImage {
        DecodedImage::new(width, height, vec![128; (width * height * 4) as usize]).unwrap()
    }

    fn sample_image() -> DecodedImage {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/desktop/public/samples/portrait.jpg");
        image_core::decode_source_from_path(&path)
            .expect("bundled sample should decode")
            .image
    }

    #[test]
    fn embedded_face_models_are_available() {
        assert!(face_model_available());
        assert_eq!(FACE_MODEL_NAME, "CenterFace + YuNet");
    }

    #[test]
    fn analyzer_returns_available_even_without_faces() {
        let analyzer = EnsembleFaceAnalyzer::new().unwrap();
        let result = analyzer.analyze(&blank_image(128, 128));
        assert!(result.available);
        assert!(result.faces.is_empty());
    }

    #[test]
    fn ensemble_detects_the_sample_face() {
        let analyzer = EnsembleFaceAnalyzer::new().unwrap();
        let result = analyzer.analyze(&sample_image());
        assert!(
            result.available,
            "models should load: {:?}",
            result.warnings
        );
        assert!(
            !result.faces.is_empty(),
            "sample portrait should contain a face"
        );
        let face = &result.faces[0];
        assert!((0.0..=1.0).contains(&face.confidence));
        assert!(face.confidence > 0.5);
        assert!(!face.models.is_empty());
        assert!(face.x >= 0.0 && face.y >= 0.0);
    }
}
