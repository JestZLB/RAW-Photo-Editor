use std::fs;
use std::io::{BufRead, Cursor, Read, Seek};
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::{
    ensure_dimensions, map_decode_error, GamutLabel, IccProfile, ImageError, ImageFileFormat,
    LinearImage, MAX_FILE_BYTES,
};

/// Decoded 8-bit sRGB image with straight (non-premultiplied) alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, 4 bytes per pixel.
    pub rgba8: Vec<u8>,
}

impl DecodedImage {
    pub fn new(width: u32, height: u32, rgba8: Vec<u8>) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        let expected = width as usize * height as usize * 4;
        if rgba8.len() != expected {
            return Err(ImageError::InvalidImage(format!(
                "RGBA buffer length {} does not match {}x{}x4",
                rgba8.len(),
                width,
                height
            )));
        }
        Ok(Self {
            width,
            height,
            rgba8,
        })
    }

    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    pub fn to_linear(&self) -> LinearImage {
        LinearImage::from_decoded(self)
    }

    fn resized(&self, width: u32, height: u32) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        if width == self.width && height == self.height {
            return Ok(self.clone());
        }
        let buffer = image::RgbaImage::from_raw(self.width, self.height, self.rgba8.clone())
            .ok_or_else(|| ImageError::InvalidImage("invalid RGBA buffer".into()))?;
        let resized = image::imageops::resize(
            &buffer,
            width,
            height,
            image::imageops::FilterType::Lanczos3,
        );
        Ok(Self {
            width,
            height,
            rgba8: resized.into_raw(),
        })
    }

    /// Resize to an exact size using Lanczos3 in display (sRGB) space. This is
    /// intentionally done before linearization to match how preview proxies are
    /// generated.
    pub fn resize_exact(&self, width: u32, height: u32) -> Result<Self, ImageError> {
        self.resized(width, height)
    }

    /// Scale to cover the target, then center-crop. Used for provider results
    /// whose aspect ratio differs from the project canvas (no stretching).
    pub fn resize_cover(&self, width: u32, height: u32) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        if self.width == width && self.height == height {
            return Ok(self.clone());
        }
        let buffer = image::RgbaImage::from_raw(self.width, self.height, self.rgba8.clone())
            .ok_or_else(|| ImageError::InvalidImage("invalid RGBA buffer".into()))?;
        let scale = (width as f64 / self.width as f64).max(height as f64 / self.height as f64);
        let scaled_width = ((self.width as f64 * scale).round() as u32)
            .max(width)
            .max(1);
        let scaled_height = ((self.height as f64 * scale).round() as u32)
            .max(height)
            .max(1);
        let resized = image::imageops::resize(
            &buffer,
            scaled_width,
            scaled_height,
            image::imageops::FilterType::Lanczos3,
        );
        let offset_x = (scaled_width - width) / 2;
        let offset_y = (scaled_height - height) / 2;
        let cropped =
            image::imageops::crop_imm(&resized, offset_x, offset_y, width, height).to_image();
        Ok(Self {
            width,
            height,
            rgba8: cropped.into_raw(),
        })
    }

    /// Fit the longest edge inside `max_dimension`, preserving aspect ratio.
    pub fn resize_to_fit(&self, max_dimension: u32) -> Result<Self, ImageError> {
        if max_dimension == 0 {
            return Ok(self.clone());
        }
        let longest = self.width.max(self.height);
        if longest <= max_dimension {
            return Ok(self.clone());
        }
        let scale = max_dimension as f64 / longest as f64;
        let width = ((self.width as f64 * scale).round() as u32).max(1);
        let height = ((self.height as f64 * scale).round() as u32).max(1);
        self.resized(width, height)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba8[offset],
            self.rgba8[offset + 1],
            self.rgba8[offset + 2],
            self.rgba8[offset + 3],
        ])
    }
}

/// Detect the on-disk image format from its signature. Used to record the
/// logical format and to choose a managed extension for byte imports.
pub fn detect_image_format(path: &Path) -> Result<ImageFileFormat, ImageError> {
    let mut file = fs::File::open(path).map_err(|e| ImageError::Io(e.to_string()))?;
    let mut header = [0u8; 64];
    let read = file
        .read(&mut header)
        .map_err(|e| ImageError::Io(e.to_string()))?;
    detect_image_format_from_memory(&header[..read])
}

pub fn detect_image_format_from_memory(bytes: &[u8]) -> Result<ImageFileFormat, ImageError> {
    let guessed =
        image::guess_format(bytes).map_err(|e| ImageError::InvalidImage(e.to_string()))?;
    ImageFileFormat::from_image_format(guessed)
        .ok_or_else(|| ImageError::UnsupportedFormat(format!("{guessed:?}")))
}

pub fn decode_image_from_path(path: &Path) -> Result<DecodedImage, ImageError> {
    Ok(decode_source_from_path(path)?.image)
}

pub fn decode_image_from_memory(bytes: &[u8]) -> Result<DecodedImage, ImageError> {
    Ok(decode_source_from_memory(bytes)?.image)
}

/// Decoded pixels plus what was learned about their colour space.
pub struct DecodedSource {
    pub image: DecodedImage,
    /// Colour space declared by the file (sRGB when nothing is embedded).
    pub source_gamut: GamutLabel,
    /// Profile description, when an embedded one was readable.
    pub profile_description: String,
    /// Set when pixels were moved out of the file's own space into the working
    /// space. Keeping the profile makes a faithful "export in the source space"
    /// possible later on.
    pub profile: Option<IccProfile>,
    /// Raw EXIF TIFF block as stored by the file, if any.
    pub exif: Option<Vec<u8>>,
}

pub fn decode_source_from_path(path: &Path) -> Result<DecodedSource, ImageError> {
    let metadata = fs::metadata(path).map_err(|e| ImageError::Io(e.to_string()))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(ImageError::FileTooLarge {
            bytes: metadata.len(),
        });
    }
    let reader = ImageReader::open(path)
        .map_err(|e| ImageError::Io(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| ImageError::Io(e.to_string()))?;
    decode_reader(reader, || crate::tiff_file_icc(path))
}

pub fn decode_source_from_memory(bytes: &[u8]) -> Result<DecodedSource, ImageError> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ImageError::FileTooLarge {
            bytes: bytes.len() as u64,
        });
    }
    let mut cursor = Cursor::new(bytes);
    // Confirm it looks like an image before constructing the reader so bogus
    // payloads produce a stable INVALID_IMAGE / UNSUPPORTED_FORMAT error.
    let guessed =
        image::guess_format(bytes).map_err(|e| ImageError::InvalidImage(e.to_string()))?;
    if ImageFileFormat::from_image_format(guessed).is_none() {
        return Err(ImageError::UnsupportedFormat(format!("{guessed:?}")));
    }
    let reader = ImageReader::new(&mut cursor)
        .with_guessed_format()
        .map_err(|e| ImageError::Io(e.to_string()))?;
    decode_reader(reader, || crate::tiff_icc_from_bytes(bytes))
}

fn decode_reader<R: BufRead + Seek>(
    reader: ImageReader<R>,
    icc_fallback: impl FnOnce() -> Option<Vec<u8>>,
) -> Result<DecodedSource, ImageError> {
    let format = reader
        .format()
        .ok_or_else(|| ImageError::InvalidImage("unknown image format".into()))?;
    let logical = ImageFileFormat::from_image_format(format)
        .ok_or_else(|| ImageError::UnsupportedFormat(format!("{format:?}")))?;
    let _ = logical;

    let mut decoder = reader.into_decoder().map_err(map_decode_error)?;
    let (width, height) = decoder.dimensions();
    ensure_dimensions(width, height)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    // The TIFF decoder cannot read the BYTE-array profile its own encoder
    // writes, so fall back to a direct tag read for TIFF only.
    let embedded = decoder.icc_profile().unwrap_or(None).or_else(|| {
        if format == image::ImageFormat::Tiff {
            icc_fallback()
        } else {
            None
        }
    });
    let exif = decoder.exif_metadata().unwrap_or(None);
    let mut dynamic = DynamicImage::from_decoder(decoder).map_err(map_decode_error)?;
    dynamic.apply_orientation(orientation);
    let rgba = dynamic.into_rgba8();
    let (width, height) = rgba.dimensions();
    ensure_dimensions(width, height)?;
    let mut image = DecodedImage::new(width, height, rgba.into_raw())?;

    let profile = embedded.as_deref().and_then(IccProfile::parse);
    let source_gamut = profile
        .as_ref()
        .map(|p| p.gamut)
        .unwrap_or(GamutLabel::Srgb);
    let profile_description = profile
        .as_ref()
        .map(|p| p.description.clone())
        .unwrap_or_default();
    let converted = match &profile {
        Some(parsed) if !parsed.matches_working_space => {
            parsed.convert_pixels(&mut image.rgba8);
            true
        }
        _ => false,
    };
    Ok(DecodedSource {
        image,
        source_gamut,
        profile_description,
        profile: if converted { profile } else { None },
        exif,
    })
}

/// Sample format label of the stored raster, e.g. `rgb8`, `rgba16`. Used to
/// prove a high bit-depth export really wrote 16-bit samples.
pub fn probe_color_type(path: &Path) -> Result<String, ImageError> {
    let reader = ImageReader::open(path)
        .map_err(|e| ImageError::Io(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| ImageError::Io(e.to_string()))?;
    let decoder = reader.into_decoder().map_err(map_decode_error)?;
    Ok(format!("{:?}", decoder.color_type()).to_ascii_lowercase())
}

/// Read only the embedded EXIF block, without decoding pixels. Used at export
/// time, when the cached source may not carry metadata any more.
pub fn read_source_exif(path: &Path) -> Result<Option<Vec<u8>>, ImageError> {
    let reader = ImageReader::open(path)
        .map_err(|e| ImageError::Io(e.to_string()))?
        .with_guessed_format()
        .map_err(|e| ImageError::Io(e.to_string()))?;
    let mut decoder = reader.into_decoder().map_err(map_decode_error)?;
    decoder
        .exif_metadata()
        .map_err(map_decode_error)
        .map(|blob| blob.filter(|blob| !blob.is_empty()))
}

/// Read only the image header. Used after AI output to record asset dimensions.
pub fn probe_image_dimensions(path: &Path) -> Result<(u32, u32), ImageError> {
    let (width, height) = image::image_dimensions(path).map_err(map_decode_error)?;
    ensure_dimensions(width, height)?;
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(width: u32, height: u32) -> DecodedImage {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[(x * 20) as u8, (y * 60) as u8, 0, 255]);
            }
        }
        DecodedImage::new(width, height, rgba).unwrap()
    }

    fn solid(color: [u8; 3]) -> DecodedImage {
        let mut pixels = Vec::new();
        for _ in 0..4 {
            pixels.extend_from_slice(&[color[0], color[1], color[2], 255]);
        }
        DecodedImage::new(2, 2, pixels).unwrap()
    }

    #[test]
    fn embedded_p3_profile_is_detected_and_pixels_are_converted() {
        let dir = tempfile::tempdir().unwrap();
        let profile = crate::build_rgb_profile(
            "Display P3",
            crate::DISPLAY_P3_PRIMARIES,
            crate::ProfileTrc::Srgb,
        )
        .unwrap();
        // A mid-range orange stored as if it were Display P3. Pure primaries
        // would not do: sRGB and P3 share the red corner after clipping.
        let source = solid([200, 120, 40]);

        for (name, format) in [
            ("p3.png", crate::OutputFormat::Png),
            ("p3.jpg", crate::OutputFormat::Jpeg),
        ] {
            let path = dir.path().join(name);
            crate::encode_atomic_profiled(&path, &source, format, 95, Some(&profile)).unwrap();
            let decoded = decode_source_from_path(&path).unwrap();
            assert_eq!(
                decoded.source_gamut,
                GamutLabel::DisplayP3,
                "{name} should report P3"
            );
            assert!(decoded.profile.is_some(), "{name} should keep the profile");
            assert_eq!(decoded.profile_description, "Display P3");
            // Reading P3 code values as sRGB would keep them; converting into
            // the working space moves the colour onto sRGB's gamut shell, which
            // for this orange means a hotter, slightly less green red.
            let pixel = decoded.image.pixel(0, 0).unwrap();
            assert_ne!(&pixel[..3], &[200, 120, 40], "{name} kept raw code values");
            assert!(
                pixel[0] > 200,
                "{name} should move towards sRGB red: {pixel:?}"
            );
            assert!(pixel[1] < 120, "{name} should shed green: {pixel:?}");
        }
    }

    /// `image`'s TIFF encoder accepts an ICC profile but the tag does not come
    /// back out, so wide-gamut exports must not silently claim TIFF support.
    #[test]
    fn tiff_round_trips_pixels_and_profile() {
        let dir = tempfile::tempdir().unwrap();
        let profile = crate::build_rgb_profile(
            "Display P3",
            crate::DISPLAY_P3_PRIMARIES,
            crate::ProfileTrc::Srgb,
        )
        .unwrap();
        let path = dir.path().join("p3.tif");
        crate::encode_atomic_profiled(
            &path,
            &solid([200, 120, 40]),
            crate::OutputFormat::Tiff,
            95,
            Some(&profile),
        )
        .unwrap();
        let decoded = decode_source_from_path(&path).unwrap();
        assert_eq!(
            decoded.source_gamut,
            GamutLabel::DisplayP3,
            "the BYTE-array profile is read back through the TIFF fallback"
        );
        // The pixels were written in Display P3, so the sRGB view differs.
        assert_ne!(decoded.image.pixel(0, 0).unwrap()[..3], [200, 120, 40]);
    }

    #[test]
    fn tiff_without_a_profile_stays_srgb() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.tif");
        crate::encode_atomic(&path, &solid([200, 120, 40]), crate::OutputFormat::Tiff, 95).unwrap();
        let decoded = decode_source_from_path(&path).unwrap();
        assert_eq!(decoded.source_gamut, GamutLabel::Srgb);
        assert_eq!(decoded.image.pixel(0, 0).unwrap()[..3], [200, 120, 40]);
    }

    #[test]
    fn srgb_profile_and_missing_profile_are_both_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let source = solid([200, 120, 40]);

        let untagged = dir.path().join("untagged.png");
        crate::encode_atomic(&untagged, &source, crate::OutputFormat::Png, 95).unwrap();
        let decoded = decode_source_from_path(&untagged).unwrap();
        assert_eq!(decoded.source_gamut, GamutLabel::Srgb);
        assert!(decoded.profile.is_none());
        assert_eq!(decoded.image.pixel(0, 0).unwrap()[..3], [200, 120, 40]);

        let srgb_profile = crate::build_rgb_profile(
            "sRGB IEC61966-2.1",
            crate::SRGB_PRIMARIES,
            crate::ProfileTrc::Srgb,
        )
        .unwrap();
        let tagged = dir.path().join("srgb.png");
        crate::encode_atomic_profiled(
            &tagged,
            &source,
            crate::OutputFormat::Png,
            95,
            Some(&srgb_profile),
        )
        .unwrap();
        let decoded = decode_source_from_path(&tagged).unwrap();
        assert_eq!(decoded.source_gamut, GamutLabel::Srgb);
        assert!(decoded.profile.is_none(), "sRGB must not be re-tagged");
        assert_eq!(decoded.image.pixel(0, 0).unwrap()[..3], [200, 120, 40]);
    }

    #[test]
    fn cover_resize_center_crops_wide_images() {
        let image = pattern(8, 4);
        let resized = image.resize_cover(4, 4).unwrap();
        assert_eq!(resized.dimensions(), (4, 4));
        for y in 0..4 {
            for x in 0..4 {
                let pixel = resized.pixel(x, y).unwrap();
                assert_eq!(pixel[0], ((x + 2) * 20) as u8);
                assert_eq!(pixel[1], (y * 60) as u8);
            }
        }
    }

    #[test]
    fn cover_resize_scales_exact_ratios_without_cropping() {
        let image = pattern(4, 2);
        let covered = image.resize_cover(8, 4).unwrap();
        let expected = image.resized(8, 4).unwrap();
        assert_eq!(covered.dimensions(), (8, 4));
        assert_eq!(covered, expected);
    }
}
