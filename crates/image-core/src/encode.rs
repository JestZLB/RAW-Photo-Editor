use std::fs;
use std::io::Write;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::tiff::TiffEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::{DecodedImage, ImageError, Srgb16Image};

/// Native-endian bytes, which is what `image` expects for 16-bit color types.
fn u16_bytes(values: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

/// Embed the profile or fail loudly: exporting a wide-gamut image untagged
/// would make other apps read it as sRGB and shift every colour.
fn attach_profile<E: ImageEncoder>(encoder: &mut E, icc: Option<&[u8]>) -> Result<(), ImageError> {
    let Some(bytes) = icc else {
        return Ok(());
    };
    encoder
        .set_icc_profile(bytes.to_vec())
        .map_err(|e| ImageError::Encode(format!("this format cannot embed an ICC profile: {e}")))
}

/// EXIF is per format: the TIFF encoder in `image` has no hook, so a metadata
/// bearing TIFF export simply comes out without it (the UI says so).
fn attach_exif<E: ImageEncoder>(encoder: &mut E, exif: Option<&[u8]>) -> Result<(), ImageError> {
    let Some(bytes) = exif else {
        return Ok(());
    };
    encoder
        .set_exif_metadata(bytes.to_vec())
        .map_err(|e| ImageError::Encode(format!("this format cannot embed EXIF: {e}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
    Tiff,
    Webp,
}

impl OutputFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
            Self::Webp => "webp",
        }
    }

    pub fn from_path(path: &Path) -> Result<Self, ImageError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .ok_or_else(|| ImageError::UnsupportedFormat("output path has no extension".into()))?;
        match ext.as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "tif" | "tiff" => Ok(Self::Tiff),
            "webp" => Ok(Self::Webp),
            other => Err(ImageError::UnsupportedFormat(other.to_string())),
        }
    }
}

/// Pixel buffer handed to the encoder, at either bit depth.
#[derive(Debug, Clone)]
pub enum RasterImage {
    Eight(DecodedImage),
    Sixteen(Srgb16Image),
}

impl RasterImage {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Eight(image) => (image.width, image.height),
            Self::Sixteen(image) => (image.width, image.height),
        }
    }

    pub fn width(&self) -> u32 {
        self.dimensions().0
    }

    pub fn height(&self) -> u32 {
        self.dimensions().1
    }
}

/// Optional sidecars written alongside the pixels.
#[derive(Debug, Clone, Default)]
pub struct EncodeOptions<'a> {
    /// ICC profile bytes; required for anything that is not sRGB.
    pub icc: Option<&'a [u8]>,
    /// Raw EXIF TIFF block, without the `Exif` identifier the encoders add.
    pub exif: Option<&'a [u8]>,
}

/// Encode to disk atomically: write a temp file in the same directory, fsync,
/// then replace the target. The parent directory is used as-is and is never
/// created, because exports must honor the chosen path.
pub fn encode_atomic(
    path: &Path,
    image: &DecodedImage,
    format: OutputFormat,
    quality: u32,
) -> Result<u64, ImageError> {
    encode_atomic_image(
        path,
        &RasterImage::Eight(image.clone()),
        format,
        quality,
        &EncodeOptions::default(),
    )
}

/// Convenience wrapper that only embeds a colour profile.
pub fn encode_atomic_profiled(
    path: &Path,
    image: &DecodedImage,
    format: OutputFormat,
    quality: u32,
    icc: Option<&[u8]>,
) -> Result<u64, ImageError> {
    encode_atomic_image(
        path,
        &RasterImage::Eight(image.clone()),
        format,
        quality,
        &EncodeOptions { icc, exif: None },
    )
}

pub fn encode_atomic_image(
    path: &Path,
    image: &RasterImage,
    format: OutputFormat,
    quality: u32,
    options: &EncodeOptions,
) -> Result<u64, ImageError> {
    if matches!(image, RasterImage::Sixteen(_))
        && matches!(format, OutputFormat::Jpeg | OutputFormat::Webp)
    {
        return Err(ImageError::UnsupportedFormat(
            "this format cannot store 16-bit samples".into(),
        ));
    }
    let path_format = OutputFormat::from_path(path)?;
    if path_format != format {
        return Err(ImageError::UnsupportedFormat(format!(
            "extension does not match requested format {}",
            format.label()
        )));
    }
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    };
    if !parent.exists() {
        return Err(ImageError::Io(format!(
            "output directory does not exist: {}",
            parent.display()
        )));
    }

    let mut temp =
        tempfile::NamedTempFile::new_in(&parent).map_err(|e| ImageError::Io(e.to_string()))?;
    let writer = temp.as_file_mut();
    let icc = options.icc;
    let exif = options.exif;
    match (format, image) {
        (OutputFormat::Png, RasterImage::Eight(image)) => {
            let mut encoder = PngEncoder::new(&mut *writer);
            attach_profile(&mut encoder, icc)?;
            attach_exif(&mut encoder, exif)?;
            encoder
                .write_image(
                    &image.rgba8,
                    image.width,
                    image.height,
                    ExtendedColorType::Rgba8,
                )
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        (OutputFormat::Png, RasterImage::Sixteen(image)) => {
            let mut encoder = PngEncoder::new(&mut *writer);
            attach_profile(&mut encoder, icc)?;
            attach_exif(&mut encoder, exif)?;
            let bytes = u16_bytes(&image.rgba);
            encoder
                .write_image(&bytes, image.width, image.height, ExtendedColorType::Rgba16)
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        (OutputFormat::Jpeg, RasterImage::Eight(image)) => {
            let mut rgb = Vec::with_capacity(image.rgba8.len() / 4 * 3);
            for pixel in image.rgba8.chunks_exact(4) {
                let alpha = pixel[3] as f32 / 255.0;
                for channel in &pixel[..3] {
                    let value = *channel as f32 * alpha + 255.0 * (1.0 - alpha);
                    rgb.push(value.round().clamp(0.0, 255.0) as u8);
                }
            }
            let mut encoder =
                JpegEncoder::new_with_quality(&mut *writer, quality.clamp(1, 100) as u8);
            attach_profile(&mut encoder, icc)?;
            attach_exif(&mut encoder, exif)?;
            encoder
                .write_image(&rgb, image.width, image.height, ExtendedColorType::Rgb8)
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        (OutputFormat::Tiff, RasterImage::Eight(image)) => {
            let mut encoder = TiffEncoder::new(&mut *writer);
            attach_profile(&mut encoder, icc)?;
            encoder
                .write_image(
                    &image.rgba8,
                    image.width,
                    image.height,
                    ExtendedColorType::Rgba8,
                )
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        (OutputFormat::Tiff, RasterImage::Sixteen(image)) => {
            let mut encoder = TiffEncoder::new(&mut *writer);
            attach_profile(&mut encoder, icc)?;
            let bytes = u16_bytes(&image.rgba);
            encoder
                .write_image(&bytes, image.width, image.height, ExtendedColorType::Rgba16)
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        (OutputFormat::Webp, RasterImage::Eight(image)) => {
            // The bundled encoder is lossless VP8L; it carries no ICC or EXIF,
            // which is why the service restricts WebP to sRGB and reports
            // metadataWritten=false.
            image::codecs::webp::WebPEncoder::new_lossless(&mut *writer)
                .encode(
                    &image.rgba8,
                    image.width,
                    image.height,
                    ExtendedColorType::Rgba8,
                )
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        (OutputFormat::Jpeg | OutputFormat::Webp, RasterImage::Sixteen(_)) => {
            unreachable!("rejected above")
        }
    }
    writer.flush().map_err(|e| ImageError::Io(e.to_string()))?;
    // The TIFF encoder cannot carry EXIF, so the finished file is rewritten in
    // place. Failure to embed is not fatal: the export result reports whether
    // metadata actually made it into the file.
    if format == OutputFormat::Tiff {
        if let Some(exif) = exif {
            let mut bytes =
                std::fs::read(temp.path()).map_err(|e| ImageError::Io(e.to_string()))?;
            if crate::metadata::write_tiff_exif(&mut bytes, exif) {
                std::fs::write(temp.path(), &bytes).map_err(|e| ImageError::Io(e.to_string()))?;
            }
        }
    }
    temp.as_file()
        .sync_all()
        .map_err(|e| ImageError::Io(e.to_string()))?;
    let bytes = temp
        .as_file()
        .metadata()
        .map_err(|e| ImageError::Io(e.to_string()))?
        .len();
    temp.persist(path)
        .map_err(|e| ImageError::Io(e.to_string()))?;
    Ok(bytes)
}

/// Convenience helper used by the asset store when it needs to ensure a
/// directory exists before encoding. Kept separate from `encode_atomic` so
/// user-chosen export paths are never silently created.
pub fn ensure_parent_dir(path: &Path) -> Result<(), ImageError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| ImageError::Io(e.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod metadata_and_depth_tests {
    use super::*;
    use crate::{linear_to_srgb_u16, LinearImage, MetadataPolicy, RasterImage};
    use image::{ImageDecoder, ImageReader};

    fn linear_ramp(width: u32, height: u32) -> LinearImage {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = (x as f32 / width as f32) * (y as f32 / height as f32);
                // A smooth ramp: 8-bit quantisation would land on ~255 steps,
                // 16-bit keeps thousands, which is what these tests assert.
                data.extend_from_slice(&[value, value * 0.5, 1.0 - value, 1.0]);
            }
        }
        LinearImage::new(width, height, data).unwrap()
    }

    /// Minimal little-endian TIFF block: IFD0 with an oriented orientation tag.
    fn exif_block(orientation: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"II*\0");
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0x0112u16.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&orientation.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    fn orientation_of(tiff: &[u8]) -> Option<u16> {
        let count = u16::from_le_bytes([*tiff.get(8)?, *tiff.get(9)?]) as usize;
        for index in 0..count {
            let entry = 10 + index * 12;
            if u16::from_le_bytes([*tiff.get(entry)?, *tiff.get(entry + 1)?]) == 0x0112 {
                return Some(u16::from_le_bytes([
                    *tiff.get(entry + 8)?,
                    *tiff.get(entry + 9)?,
                ]));
            }
        }
        None
    }

    fn color_type_of(path: &Path) -> image::ColorType {
        let reader = ImageReader::open(path)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        let decoder = reader.into_decoder().unwrap();
        decoder.color_type()
    }

    #[test]
    fn sixteen_bit_png_and_tiff_keep_more_than_256_steps() {
        let dir = tempfile::tempdir().unwrap();
        let image = linear_ramp(64, 48).to_srgb16();
        assert_eq!(image.dimensions(), (64, 48));
        // The ramp must actually use the wider range.
        let distinct: std::collections::HashSet<u16> =
            image.rgba.chunks_exact(4).map(|pixel| pixel[0]).collect();
        assert!(
            distinct.len() > 300,
            "expected a 16-bit ramp, got {} steps",
            distinct.len()
        );

        for (name, format) in [
            ("ramp.png", OutputFormat::Png),
            ("ramp.tif", OutputFormat::Tiff),
        ] {
            let path = dir.path().join(name);
            let bytes = encode_atomic_image(
                &path,
                &RasterImage::Sixteen(image.clone()),
                format,
                100,
                &EncodeOptions::default(),
            )
            .unwrap();
            assert!(bytes > 0);
            assert_eq!(color_type_of(&path), image::ColorType::Rgba16, "{name}");
            // Decoding to 8-bit still works for ordinary consumers.
            let decoded = crate::decode_image_from_path(&path).unwrap();
            assert_eq!(decoded.dimensions(), (64, 48));
        }
    }

    #[test]
    fn jpeg_refuses_sixteen_bit_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ramp.jpg");
        let image = linear_ramp(8, 8).to_srgb16();
        let error = encode_atomic_image(
            &path,
            &RasterImage::Sixteen(image),
            OutputFormat::Jpeg,
            90,
            &EncodeOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_FORMAT");
        assert!(!path.exists(), "a rejected export must not create a file");
    }

    #[test]
    fn exif_survives_jpeg_and_png_export_with_orientation_normalised() {
        let dir = tempfile::tempdir().unwrap();
        let source = exif_block(6);
        let prepared = crate::prepare_exif(&source, MetadataPolicy::Keep).unwrap();
        assert_eq!(orientation_of(&prepared), Some(1));
        assert_eq!(
            orientation_of(&source),
            Some(6),
            "the source stays untouched"
        );

        let pixels = linear_ramp(16, 16).to_srgb8();
        for (name, format) in [
            ("meta.jpg", OutputFormat::Jpeg),
            ("meta.png", OutputFormat::Png),
        ] {
            let path = dir.path().join(name);
            encode_atomic_image(
                &path,
                &RasterImage::Eight(pixels.clone()),
                format,
                95,
                &EncodeOptions {
                    icc: None,
                    exif: Some(&prepared),
                },
            )
            .unwrap();
            let decoded = crate::decode_source_from_path(&path).unwrap();
            let written = decoded.exif.expect("export should carry EXIF");
            // Encoders may or may not keep the `Exif` identifier; find the TIFF
            // header and read the orientation from there.
            let start = written
                .windows(2)
                .position(|pair| pair == b"II")
                .expect("a TIFF block");
            assert_eq!(
                orientation_of(&written[start..]),
                Some(1),
                "{name} orientation"
            );
        }
    }

    #[test]
    fn policy_none_and_privacy_are_distinct() {
        let mut block = exif_block(8);
        // Add a GPSInfo pointer so the privacy policy has something to drop.
        let count = 2u16;
        block.splice(8..10, count.to_le_bytes());
        let entry = 10 + 12;
        let mut gps = Vec::new();
        gps.extend_from_slice(&0x8825u16.to_le_bytes());
        gps.extend_from_slice(&4u16.to_le_bytes());
        gps.extend_from_slice(&1u32.to_le_bytes());
        gps.extend_from_slice(&4096u32.to_le_bytes());
        block.splice(entry..entry, gps);

        assert!(crate::has_gps(&block));
        assert!(crate::prepare_exif(&block, MetadataPolicy::None).is_none());
        assert!(crate::has_gps(
            &crate::prepare_exif(&block, MetadataPolicy::Keep).unwrap()
        ));
        assert!(!crate::has_gps(
            &crate::prepare_exif(&block, MetadataPolicy::Privacy).unwrap()
        ));
    }

    #[test]
    fn to_srgb16_matches_the_eight_bit_path_at_coarse_steps() {
        let image = LinearImage::new(1, 1, vec![0.5, 0.0, 1.0, 1.0]).unwrap();
        let eight = image.to_srgb8();
        let sixteen = image.to_srgb16();
        let widened = (sixteen.rgba[0] as f64 / 65535.0 * 255.0).round() as i32;
        assert!((widened - eight.rgba8[0] as i32).abs() <= 1);
        assert_eq!(sixteen.rgba[3], 65535);
        assert_eq!(linear_to_srgb_u16(0.0), 0);
        assert_eq!(linear_to_srgb_u16(f32::NAN), 0);
    }
}
