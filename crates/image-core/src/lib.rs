//! Image decoding/encoding, sRGB <-> linear conversion and pixel operations.

mod blend;
mod decoded;
mod encode;
mod icc;
mod linear;
mod metadata;
mod watermark;

pub use blend::*;
pub use decoded::*;
pub use encode::*;
pub use icc::*;
pub use linear::*;
pub use metadata::*;
pub use watermark::*;

use thiserror::Error;

pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 100_000_000;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("invalid image: {0}")]
    InvalidImage(String),
    #[error("unsupported image format: {0}")]
    UnsupportedFormat(String),
    #[error("image is too large: {width}x{height}")]
    ImageTooLarge { width: u32, height: u32 },
    #[error("file is too large: {bytes} bytes")]
    FileTooLarge { bytes: u64 },
    #[error("failed to encode image: {0}")]
    Encode(String),
}

impl ImageError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "IO",
            Self::InvalidImage(_) => "INVALID_IMAGE",
            Self::UnsupportedFormat(_) => "UNSUPPORTED_FORMAT",
            Self::ImageTooLarge { .. } | Self::FileTooLarge { .. } => "IMAGE_TOO_LARGE",
            Self::Encode(_) => "EXPORT_FAILED",
        }
    }

    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Io(_) => "无法读取或写入图片文件，请确认文件路径和权限。",
            Self::InvalidImage(_) => "图片文件已损坏，或不是可识别的图片。",
            Self::UnsupportedFormat(_) => "不支持的图片格式，请使用 JPEG、PNG、TIFF 或 WebP 格式。",
            Self::ImageTooLarge { .. } | Self::FileTooLarge { .. } => {
                "图片超出限制：最大 100 百万像素、32MB 文件。"
            }
            Self::Encode(_) => "图片保存失败，请检查目标目录和磁盘空间。",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileFormat {
    Jpeg,
    Png,
    Tiff,
    WebP,
}

impl ImageFileFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Tiff => "tiff",
            Self::WebP => "webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Tiff => "tiff",
            Self::WebP => "webp",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "tif" | "tiff" => Some(Self::Tiff),
            "webp" => Some(Self::WebP),
            _ => None,
        }
    }

    pub fn from_image_format(format: image::ImageFormat) -> Option<Self> {
        match format {
            image::ImageFormat::Jpeg => Some(Self::Jpeg),
            image::ImageFormat::Png => Some(Self::Png),
            image::ImageFormat::Tiff => Some(Self::Tiff),
            image::ImageFormat::WebP => Some(Self::WebP),
            _ => None,
        }
    }

    pub fn supported_extensions() -> &'static [&'static str] {
        &["jpg", "jpeg", "png", "tif", "tiff", "webp"]
    }
}

pub(crate) fn ensure_dimensions(width: u32, height: u32) -> Result<(), ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::InvalidImage(
            "image dimensions must be non-zero".into(),
        ));
    }
    if width as u64 * height as u64 > MAX_PIXELS {
        return Err(ImageError::ImageTooLarge { width, height });
    }
    Ok(())
}

pub(crate) fn map_decode_error(error: image::ImageError) -> ImageError {
    use image::ImageError as E;
    match error {
        E::IoError(e) => ImageError::Io(e.to_string()),
        E::Unsupported(e) => ImageError::UnsupportedFormat(e.to_string()),
        E::Limits(_) => ImageError::ImageTooLarge {
            width: 0,
            height: 0,
        },
        E::Decoding(e) => ImageError::InvalidImage(e.to_string()),
        E::Encoding(e) => ImageError::Encode(e.to_string()),
        E::Parameter(e) => ImageError::InvalidImage(e.to_string()),
    }
}

/// Convert a linear sRGB channel to an 8-bit display value.
pub fn linear_to_srgb_u8(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    let encoded = linear_to_srgb(value.clamp(0.0, 1.0));
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Same transfer function quantized to 16 bits, used by high bit-depth exports.
pub fn linear_to_srgb_u16(value: f32) -> u16 {
    if !value.is_finite() {
        return 0;
    }
    let encoded = linear_to_srgb(value.clamp(0.0, 1.0));
    (encoded * 65535.0).round().clamp(0.0, 65535.0) as u16
}

/// sRGB transfer function (display -> linear).
pub fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse sRGB transfer function (linear -> display). Values outside 0..1 are
/// allowed to preserve highlight headroom until the final encode step.
pub fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_u8_to_linear(value: u8) -> f32 {
    srgb_to_linear(value as f32 / 255.0)
}

/// 16-bit counterpart of [`srgb_u8_to_linear`].
pub fn srgb_u16_to_linear(value: u16) -> f32 {
    srgb_to_linear(value as f32 / 65535.0)
}
