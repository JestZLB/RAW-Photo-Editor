use rayon::prelude::*;

use crate::{
    ensure_dimensions, linear_to_srgb_u16, linear_to_srgb_u8, srgb_u8_to_linear, DecodedImage,
    ImageError,
};

/// Linear-light RGBA image in sRGB primaries. Alpha is straight, 0..1.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    /// RGBA f32, row-major, linear light.
    pub data: Vec<f32>,
}

impl LinearImage {
    pub fn new(width: u32, height: u32, data: Vec<f32>) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        let expected = width as usize * height as usize * 4;
        if data.len() != expected {
            return Err(ImageError::InvalidImage(format!(
                "linear buffer length {} does not match {}x{}x4",
                data.len(),
                width,
                height
            )));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn transparent(width: u32, height: u32) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        Ok(Self {
            width,
            height,
            data: vec![0.0; width as usize * height as usize * 4],
        })
    }

    /// Overwrite with another image's pixels while keeping this buffer's
    /// allocation. Layer compositing needs a scratch copy per layer; allocating
    /// a fresh one each time doubles peak memory on large exports.
    pub fn copy_from(&mut self, other: &LinearImage) {
        self.width = other.width;
        self.height = other.height;
        self.data.clear();
        self.data.extend_from_slice(&other.data);
    }

    /// Convert 8-bit sRGB pixels into linear light, preserving alpha exactly.
    pub fn from_decoded(decoded: &DecodedImage) -> Self {
        let pixel_count = decoded.width as usize * decoded.height as usize;
        let mut data = vec![0.0f32; pixel_count * 4];
        data.par_chunks_mut(4)
            .zip(decoded.rgba8.par_chunks(4))
            .for_each(|(out, src)| {
                out[0] = srgb_u8_to_linear(src[0]);
                out[1] = srgb_u8_to_linear(src[1]);
                out[2] = srgb_u8_to_linear(src[2]);
                out[3] = src[3] as f32 / 255.0;
            });
        Self {
            width: decoded.width,
            height: decoded.height,
            data,
        }
    }

    /// Convert back to 8-bit sRGB. RGB values are clamped to 0..1; alpha is
    /// preserved. Used only at the final encode boundary.
    pub fn to_srgb8(&self) -> DecodedImage {
        let mut rgba8 = vec![0u8; self.data.len()];
        rgba8
            .par_chunks_mut(4)
            .zip(self.data.par_chunks(4))
            .for_each(|(out, src)| {
                out[0] = linear_to_srgb_u8(src[0]);
                out[1] = linear_to_srgb_u8(src[1]);
                out[2] = linear_to_srgb_u8(src[2]);
                let alpha = if src[3].is_finite() {
                    (src[3].clamp(0.0, 1.0) * 255.0).round() as u8
                } else {
                    0
                };
                out[3] = alpha;
            });
        DecodedImage {
            width: self.width,
            height: self.height,
            rgba8,
        }
    }

    /// Quantize to 16-bit sRGB. Only meaningful after a linear (f32) render:
    /// upscaling an 8-bit buffer would fake the extra precision.
    pub fn to_srgb16(&self) -> Srgb16Image {
        let mut rgba = vec![0u16; self.data.len()];
        rgba.par_chunks_mut(4)
            .zip(self.data.par_chunks(4))
            .for_each(|(out, src)| {
                out[0] = linear_to_srgb_u16(src[0]);
                out[1] = linear_to_srgb_u16(src[1]);
                out[2] = linear_to_srgb_u16(src[2]);
                out[3] = if src[3].is_finite() {
                    (src[3].clamp(0.0, 1.0) * 65535.0).round() as u16
                } else {
                    0
                };
            });
        Srgb16Image {
            width: self.width,
            height: self.height,
            rgba,
        }
    }

    /// Scale then centre-crop to an exact size, mirroring `DecodedImage::resize_cover`
    /// but staying in linear light so 16-bit exports keep their precision.
    pub fn resize_cover(&self, width: u32, height: u32) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        if self.width == width && self.height == height {
            return Ok(self.clone());
        }
        let scale = (width as f64 / self.width as f64).max(height as f64 / self.height as f64);
        let scaled_width = ((self.width as f64 * scale).round() as u32)
            .max(width)
            .max(1);
        let scaled_height = ((self.height as f64 * scale).round() as u32)
            .max(height)
            .max(1);
        let resized = self.resize_lanczos3(scaled_width, scaled_height)?;
        let offset_x = (scaled_width - width) / 2;
        let offset_y = (scaled_height - height) / 2;
        resized.crop(offset_x, offset_y, width, height)
    }

    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    pub fn index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((y as usize * self.width as usize + x as usize) * 4)
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[f32; 4]> {
        let idx = self.index(x, y)?;
        Some([
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ])
    }

    /// Crop a rectangle in pixels without resampling. Alpha is preserved.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        if x.checked_add(width).is_none_or(|end| end > self.width)
            || y.checked_add(height).is_none_or(|end| end > self.height)
        {
            return Err(ImageError::InvalidImage(format!(
                "crop {}x{}+{}+{} exceeds {}x{}",
                width, height, x, y, self.width, self.height
            )));
        }
        if x == 0 && y == 0 && width == self.width && height == self.height {
            return Ok(self.clone());
        }
        let src_width = self.width as usize;
        let dst_width = width as usize;
        let mut out = vec![0.0f32; dst_width * height as usize * 4];
        out.par_chunks_mut(dst_width * 4)
            .enumerate()
            .for_each(|(row, out_row)| {
                let src_start = ((y as usize + row) * src_width + x as usize) * 4;
                let src_end = src_start + dst_width * 4;
                out_row.copy_from_slice(&self.data[src_start..src_end]);
            });
        Self::new(width, height, out)
    }

    /// Separable Lanczos3 resize in linear light, used for AI result assets
    /// before compositing. It does not touch the display-space source proxy.
    pub fn resize_lanczos3(&self, width: u32, height: u32) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        if width == self.width && height == self.height {
            return Ok(self.clone());
        }
        if width == self.width {
            return self.resize_vertical(height);
        }
        if height == self.height {
            return self.resize_horizontal(width);
        }

        let src_width = self.width as usize;
        let src_height = self.height as usize;
        let dst_width = width as usize;
        let dst_height = height as usize;

        let mut horizontal = vec![0.0f32; dst_width * src_height * 4];
        let h_weights = lanczos_weights(src_width, dst_width);
        horizontal
            .par_chunks_mut(dst_width * 4)
            .enumerate()
            .for_each(|(y, out_row)| {
                let src_row = &self.data[y * src_width * 4..(y + 1) * src_width * 4];
                for (x, weights) in h_weights.iter().enumerate() {
                    let mut acc = [0.0f32; 4];
                    for &(sx, w) in weights {
                        let base = sx * 4;
                        acc[0] += src_row[base] * w;
                        acc[1] += src_row[base + 1] * w;
                        acc[2] += src_row[base + 2] * w;
                        acc[3] += src_row[base + 3] * w;
                    }
                    let base = x * 4;
                    out_row[base..base + 4].copy_from_slice(&acc);
                }
            });

        let mut out = vec![0.0f32; dst_width * dst_height * 4];
        let v_weights = lanczos_weights(src_height, dst_height);
        out.par_chunks_mut(dst_width * 4)
            .enumerate()
            .for_each(|(y, out_row)| {
                let weights = &v_weights[y];
                for x in 0..dst_width {
                    let mut acc = [0.0f32; 4];
                    for &(sy, w) in weights {
                        let base = (sy * dst_width + x) * 4;
                        acc[0] += horizontal[base] * w;
                        acc[1] += horizontal[base + 1] * w;
                        acc[2] += horizontal[base + 2] * w;
                        acc[3] += horizontal[base + 3] * w;
                    }
                    let base = x * 4;
                    out_row[base..base + 4].copy_from_slice(&acc);
                }
            });

        Self::new(width, height, out)
    }

    fn resize_horizontal(&self, width: u32) -> Result<Self, ImageError> {
        let src_width = self.width as usize;
        let height = self.height as usize;
        let dst_width = width as usize;
        let weights = lanczos_weights(src_width, dst_width);
        let mut out = vec![0.0f32; dst_width * height * 4];
        out.par_chunks_mut(dst_width * 4)
            .enumerate()
            .for_each(|(y, out_row)| {
                let src_row = &self.data[y * src_width * 4..(y + 1) * src_width * 4];
                for (x, weights) in weights.iter().enumerate() {
                    let mut acc = [0.0f32; 4];
                    for &(sx, w) in weights {
                        let base = sx * 4;
                        acc[0] += src_row[base] * w;
                        acc[1] += src_row[base + 1] * w;
                        acc[2] += src_row[base + 2] * w;
                        acc[3] += src_row[base + 3] * w;
                    }
                    let base = x * 4;
                    out_row[base..base + 4].copy_from_slice(&acc);
                }
            });
        Self::new(width, self.height, out)
    }

    fn resize_vertical(&self, height: u32) -> Result<Self, ImageError> {
        let width = self.width as usize;
        let src_height = self.height as usize;
        let dst_height = height as usize;
        let weights = lanczos_weights(src_height, dst_height);
        let mut out = vec![0.0f32; width * dst_height * 4];
        out.par_chunks_mut(width * 4)
            .enumerate()
            .for_each(|(y, out_row)| {
                let weights = &weights[y];
                for x in 0..width {
                    let mut acc = [0.0f32; 4];
                    for &(sy, w) in weights {
                        let base = (sy * width + x) * 4;
                        acc[0] += self.data[base] * w;
                        acc[1] += self.data[base + 1] * w;
                        acc[2] += self.data[base + 2] * w;
                        acc[3] += self.data[base + 3] * w;
                    }
                    let base = x * 4;
                    out_row[base..base + 4].copy_from_slice(&acc);
                }
            });
        Self::new(self.width, height, out)
    }
}

fn lanczos3(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        1.0
    } else if x.abs() < 3.0 {
        let pix = std::f32::consts::PI * x;
        (pix.sin() / pix) * ((pix / 3.0).sin() / (pix / 3.0))
    } else {
        0.0
    }
}

fn lanczos_weights(src_len: usize, dst_len: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = src_len as f32 / dst_len as f32;
    let filter_scale = scale.max(1.0);
    let support = 3.0 * filter_scale;
    let mut all = Vec::with_capacity(dst_len);
    for out in 0..dst_len {
        let center = (out as f32 + 0.5) * scale - 0.5;
        let start = (center - support).floor() as i64;
        let end = (center + support).ceil() as i64;
        let mut weights: Vec<(usize, f32)> = Vec::new();
        let mut sum = 0.0f32;
        for i in start..=end {
            let clamped = i.clamp(0, src_len as i64 - 1) as usize;
            let distance = (i as f32 - center) / filter_scale;
            let weight = lanczos3(distance);
            if weight == 0.0 {
                continue;
            }
            sum += weight;
            if let Some(existing) = weights.iter_mut().find(|(idx, _)| *idx == clamped) {
                existing.1 += weight;
            } else {
                weights.push((clamped, weight));
            }
        }
        if sum.abs() > 1e-6 {
            for (_, w) in &mut weights {
                *w /= sum;
            }
        }
        all.push(weights);
    }
    all
}

impl LinearImage {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// 16-bit sRGB RGBA image, used only for high bit-depth exports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Srgb16Image {
    pub width: u32,
    pub height: u32,
    /// RGBA16, row-major, 2 bytes per channel.
    pub rgba: Vec<u16>,
}

impl Srgb16Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u16>) -> Result<Self, ImageError> {
        ensure_dimensions(width, height)?;
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(ImageError::InvalidImage(format!(
                "RGBA16 buffer length {} does not match {}x{}x4",
                rgba.len(),
                width,
                height
            )));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_atomic;
    use crate::OutputFormat;

    fn sample() -> DecodedImage {
        let mut rgba = Vec::new();
        for y in 0..32u32 {
            for x in 0..32u32 {
                rgba.extend_from_slice(&[
                    (x * 8) as u8,
                    (y * 8) as u8,
                    ((x + y) * 4) as u8,
                    if (x + y) % 2 == 0 { 255 } else { 128 },
                ]);
            }
        }
        DecodedImage::new(32, 32, rgba).unwrap()
    }

    #[test]
    fn linear_roundtrip_tolerance() {
        for value in [0u8, 1, 7, 15, 63, 127, 191, 254, 255] {
            let linear = srgb_u8_to_linear(value);
            let back = linear_to_srgb_u8(linear);
            let diff = (back as i16 - value as i16).abs();
            assert!(diff <= 1, "{value} -> {linear} -> {back}");
        }
        let image = sample();
        let linear = image.to_linear();
        let back = linear.to_srgb8();
        assert_eq!(back.width, image.width);
        assert_eq!(back.height, image.height);
        for (a, b) in image.rgba8.iter().zip(back.rgba8.iter()) {
            assert!((*a as i16 - *b as i16).abs() <= 1);
        }
    }

    #[test]
    fn resize_dimensions() {
        let decoded = sample();
        assert_eq!(decoded.resize_to_fit(16).unwrap().dimensions(), (16, 16));
        let wide = DecodedImage::new(100, 50, vec![0; 100 * 50 * 4]).unwrap();
        assert_eq!(wide.resize_to_fit(20).unwrap().dimensions(), (20, 10));
        let tall = DecodedImage::new(30, 100, vec![0; 30 * 100 * 4]).unwrap();
        assert_eq!(tall.resize_to_fit(20).unwrap().dimensions(), (6, 20));
        assert_eq!(tall.resize_to_fit(0).unwrap().dimensions(), (30, 100));
    }

    #[test]
    fn linear_resize_dimensions() {
        let linear = sample().to_linear();
        let resized = linear.resize_lanczos3(10, 7).unwrap();
        assert_eq!(resized.dimensions(), (10, 7));
        assert_eq!(resized.data.len(), 10 * 7 * 4);
        assert!(resized.data.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn crop_extracts_rows_without_resampling() {
        let mut data = Vec::new();
        for y in 0..3u32 {
            for x in 0..4u32 {
                data.extend_from_slice(&[y as f32, x as f32, 0.5, 1.0]);
            }
        }
        let image = LinearImage::new(4, 3, data).unwrap();
        let cropped = image.crop(1, 1, 2, 2).unwrap();
        assert_eq!(cropped.dimensions(), (2, 2));
        assert_eq!(cropped.pixel(0, 0).unwrap(), [1.0, 1.0, 0.5, 1.0]);
        assert_eq!(cropped.pixel(1, 1).unwrap(), [2.0, 2.0, 0.5, 1.0]);
        assert_eq!(image.crop(0, 0, 4, 3).unwrap(), image);
        assert!(image.crop(3, 0, 2, 1).is_err());
    }

    #[test]
    fn blend_opacity_zero_is_noop() {
        let mut dst = sample().to_linear();
        let src = LinearImage::new(32, 32, vec![0.5; 32 * 32 * 4]).unwrap();
        let before = dst.data.clone();
        crate::composite_over(&mut dst, &src, 0.0, crate::BlendMode::Multiply).unwrap();
        assert_eq!(dst.data, before);
    }

    #[test]
    fn blur_radius_zero_is_noop() {
        let image = sample().to_linear();
        assert_eq!(crate::box_blur(&image, 0).data, image.data);
    }

    #[test]
    fn encode_png_roundtrip_and_extension_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        let image = sample();
        let bytes = encode_atomic(&path, &image, OutputFormat::Png, 90).unwrap();
        assert!(bytes > 0);
        let decoded = crate::decode_image_from_path(&path).unwrap();
        assert_eq!(decoded.dimensions(), (32, 32));
        let bad = dir.path().join("out.jpg");
        let err = encode_atomic(&bad, &image, OutputFormat::Png, 90).unwrap_err();
        assert_eq!(err.code(), "UNSUPPORTED_FORMAT");
    }
}
