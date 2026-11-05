//! Basic RAW decoding via `rawler`.
//!
//! This is an initial integration: camera coverage depends on rawler's camera
//! database and has not been validated against a per-camera sample matrix. The
//! UI reports this limitation instead of claiming verified support.

use std::path::Path;

use image_core::{DecodedImage, ImageError};
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "nef", "nrw", "arw", "dng", "raf", "rw2", "rwl", "orf", "pef", "srw", "raw",
];
pub const SUPPORTED_BRANDS: &[&str] = &["Panasonic", "Nikon", "Sony"];

/// Cameras used for real-sample verification of import -> preview -> export.
pub const VERIFIED_SAMPLES: &[(&str, &str, &str)] = &[
    ("Panasonic", "DC-FZ80", "RW2"),
    ("Nikon", "1 J1", "NEF"),
    ("Sony", "DSC-RX100", "ARW"),
];

fn verified_camera_labels() -> Vec<String> {
    VERIFIED_SAMPLES
        .iter()
        .map(|(make, model, format)| format!("{make} {model} ({format})"))
        .collect()
}
pub const MAX_RAW_FILE_BYTES: u64 = 200 * 1024 * 1024;

pub fn raw_extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    RAW_EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

pub fn is_raw_path(path: &Path) -> bool {
    raw_extension(path).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawCameraSupport {
    pub make: String,
    pub model: String,
    pub formats: Vec<String>,
}

/// Empty until camera samples have been verified. Do not fill this with
/// unverified guesses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSupportMatrix {
    pub cameras: Vec<RawCameraSupport>,
}

impl RawSupportMatrix {
    pub fn is_empty(&self) -> bool {
        self.cameras.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawCapability {
    pub available: bool,
    pub extensions: Vec<String>,
    pub brands: Vec<String>,
    /// Verified camera models. Empty until real samples are checked.
    pub supported_cameras: Vec<String>,
    pub message: String,
}

pub fn capability() -> RawCapability {
    RawCapability {
        available: true,
        extensions: RAW_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
        brands: SUPPORTED_BRANDS.iter().map(|s| s.to_string()).collect(),
        supported_cameras: verified_camera_labels(),
        message: "基础 RAW 解码已接入（rawler）：松下 RW2/RWL、尼康 NEF/NRW、索尼 ARW；已用松下 DC-FZ80、尼康 1 J1、索尼 DSC-RX100 样片验证导入/预览/导出，其他机型取决于相机数据库，导入失败时请先导出为 TIFF。"
            .to_string(),
    }
}

#[derive(Debug, Clone, Error)]
pub enum RawError {
    #[error("RAW extension is not supported: {0}")]
    Unsupported(String),
    #[error("RAW file is too large: {0} bytes")]
    FileTooLarge(u64),
    #[error("RAW decode failed: {0}")]
    Decode(String),
}

impl RawError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "RAW_UNSUPPORTED",
            Self::FileTooLarge(_) => "IMAGE_TOO_LARGE",
            Self::Decode(_) => "RAW_DECODE_FAILED",
        }
    }

    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => {
                "当前版本不支持这个 RAW 格式。请使用相机软件导出为 TIFF 后再导入。"
            }
            Self::FileTooLarge(_) => "RAW 文件超过 200 MB，请先转成 TIFF 或其他格式。",
            Self::Decode(_) => {
                "这个 RAW 文件暂时无法解码。请确认文件完整，或先用相机软件导出为 TIFF。"
            }
        }
    }
}

pub trait RawDecoder: Send + Sync {
    fn name(&self) -> &'static str;
    fn support_matrix(&self) -> RawSupportMatrix;
    fn decode(&self, path: &Path) -> Result<DecodedImage, RawError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RawlerDecoder;

impl RawDecoder for RawlerDecoder {
    fn name(&self) -> &'static str {
        "rawler"
    }

    fn support_matrix(&self) -> RawSupportMatrix {
        RawSupportMatrix {
            cameras: VERIFIED_SAMPLES
                .iter()
                .map(|(make, model, format)| RawCameraSupport {
                    make: (*make).to_string(),
                    model: (*model).to_string(),
                    formats: vec![(*format).to_string()],
                })
                .collect(),
        }
    }

    fn decode(&self, path: &Path) -> Result<DecodedImage, RawError> {
        if !is_raw_path(path) {
            return Err(RawError::Unsupported(
                path.extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            ));
        }
        let bytes = std::fs::metadata(path)
            .map_err(|error| RawError::Decode(error.to_string()))?
            .len();
        if bytes > MAX_RAW_FILE_BYTES {
            return Err(RawError::FileTooLarge(bytes));
        }
        let mut dynamic = rawler::analyze::raw_to_srgb(path, &RawDecodeParams::default())
            .map_err(|error| RawError::Decode(error.to_string()))?;
        apply_exif_orientation(&mut dynamic, raw_orientation(path));
        let rgba = dynamic.into_rgba8();
        let (width, height) = rgba.dimensions();
        if width == 0 || height == 0 {
            return Err(RawError::Decode("decoded image is empty".into()));
        }
        if width as u64 * height as u64 > 100_000_000 {
            return Err(RawError::Decode(format!(
                "decoded RAW is too large: {width}x{height}"
            )));
        }
        DecodedImage::new(width, height, rgba.into_raw())
            .map_err(|error: ImageError| RawError::Decode(error.to_string()))
    }
}

pub fn decode_raw(path: &Path) -> Result<DecodedImage, RawError> {
    RawlerDecoder.decode(path)
}

/// EXIF orientation stored in the RAW container. rawler's sRGB conversion
/// keeps sensor orientation, so it has to be applied explicitly.
pub fn raw_orientation(path: &Path) -> Option<u16> {
    let source = RawSource::new(path).ok()?;
    let decoder = rawler::get_decoder(&source).ok()?;
    decoder
        .raw_metadata(&source, &RawDecodeParams::default())
        .ok()?
        .exif
        .orientation
}

fn apply_exif_orientation(dynamic: &mut image::DynamicImage, orientation: Option<u16>) {
    let Some(value) = orientation.and_then(|value| u8::try_from(value).ok()) else {
        return;
    };
    if let Some(rotation) = image::metadata::Orientation::from_exif(value) {
        dynamic.apply_orientation(rotation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_panasonic_nikon_sony_raw_extensions_case_insensitively() {
        assert!(is_raw_path(Path::new("C:/photos/IMG_0001.CR3")));
        assert!(is_raw_path(Path::new("photo.nef")));
        assert!(is_raw_path(Path::new("photo.NRW")));
        assert!(is_raw_path(Path::new("photo.ARW")));
        assert!(is_raw_path(Path::new("photo.rw2")));
        assert!(is_raw_path(Path::new("panasonic.RWL")));
        assert!(is_raw_path(Path::new("photo.DNG")));
        assert!(!is_raw_path(Path::new("photo.jpg")));
        assert_eq!(raw_extension(Path::new("a.RW2")).as_deref(), Some("rw2"));
    }

    #[test]
    fn raw_capability_lists_verified_samples_and_brands() {
        let capability = capability();
        assert!(capability.available);
        assert!(capability.extensions.contains(&"dng".to_string()));
        assert!(capability.extensions.contains(&"nrw".to_string()));
        assert!(capability.extensions.contains(&"rwl".to_string()));
        assert_eq!(capability.brands, vec!["Panasonic", "Nikon", "Sony"]);
        assert_eq!(capability.supported_cameras.len(), 3);
        assert!(capability
            .supported_cameras
            .iter()
            .any(|item| item.contains("Panasonic DC-FZ80")));
        assert!(capability.message.contains("松下 RW2/RWL"));
        assert!(capability.message.contains("尼康 NEF/NRW"));
        assert!(capability.message.contains("索尼 ARW"));
        assert!(capability.message.contains("样片验证"));
        assert_eq!(RawlerDecoder.name(), "rawler");
        assert_eq!(RawlerDecoder.support_matrix().cameras.len(), 3);
    }

    #[test]
    fn missing_raw_file_reports_structured_decode_error() {
        let err = decode_raw(Path::new("C:/definitely-missing/sample.cr2")).unwrap_err();
        assert_eq!(err.code(), "RAW_DECODE_FAILED");
        assert!(err.user_message().contains("RAW"));
    }

    #[test]
    #[ignore = "requires a real camera RAW sample; set LUMA_RAW_SAMPLE"]
    fn decodes_real_raw_sample_when_provided() {
        let path = std::env::var("LUMA_RAW_SAMPLE").expect("set LUMA_RAW_SAMPLE");
        let image = decode_raw(Path::new(&path)).unwrap();
        assert!(image.width > 0 && image.height > 0);
        assert_eq!(
            image.rgba8.len(),
            image.width as usize * image.height as usize * 4
        );
    }
}
