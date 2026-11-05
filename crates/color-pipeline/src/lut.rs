use image_core::{linear_to_srgb, srgb_to_linear, LinearImage};

#[derive(Debug, Clone, PartialEq)]
pub enum Lut {
    OneD(Vec<[f32; 3]>),
    ThreeD { size: usize, data: Vec<[f32; 3]> },
}

impl Lut {
    pub fn dimension_label(&self) -> &'static str {
        match self {
            Self::OneD(_) => "1D",
            Self::ThreeD { .. } => "3D",
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Self::OneD(values) => values.len(),
            Self::ThreeD { size, .. } => *size,
        }
    }
}

fn parse_float(token: &str) -> Option<f32> {
    token.parse::<f32>().ok().filter(|value| value.is_finite())
}

/// Parse a `.cube` LUT. Supports `LUT_1D_SIZE` and `LUT_3D_SIZE`, ignores
/// titles and domain metadata, and validates the expected number of entries.
pub fn parse_cube(source: &[u8]) -> Result<Lut, String> {
    let text = String::from_utf8_lossy(source);
    let mut one_d_size = None;
    let mut three_d_size = None;
    let mut entries = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("TITLE") || upper.starts_with("DOMAIN") {
            continue;
        }
        if let Some(value) = upper.strip_prefix("LUT_1D_SIZE") {
            let size = value
                .trim()
                .parse::<usize>()
                .map_err(|_| "invalid 1D size")?;
            if !(2..=256).contains(&size) {
                return Err("1D LUT size must be between 2 and 256".into());
            }
            one_d_size = Some(size);
            continue;
        }
        if let Some(value) = upper.strip_prefix("LUT_3D_SIZE") {
            let size = value
                .trim()
                .parse::<usize>()
                .map_err(|_| "invalid 3D size")?;
            if !(2..=64).contains(&size) {
                return Err("3D LUT size must be between 2 and 64".into());
            }
            three_d_size = Some(size);
            continue;
        }
        let values: Vec<f32> = line.split_whitespace().filter_map(parse_float).collect();
        if values.len() >= 3 {
            entries.push([
                values[0].clamp(0.0, 1.0),
                values[1].clamp(0.0, 1.0),
                values[2].clamp(0.0, 1.0),
            ]);
        }
    }

    if let Some(size) = one_d_size {
        if entries.len() != size {
            return Err(format!(
                "1D LUT expects {size} entries, found {}",
                entries.len()
            ));
        }
        return Ok(Lut::OneD(entries));
    }
    if let Some(size) = three_d_size {
        let expected = size.checked_pow(3).ok_or("3D LUT is too large")?;
        if entries.len() != expected {
            return Err(format!(
                "3D LUT expects {expected} entries, found {}",
                entries.len()
            ));
        }
        return Ok(Lut::ThreeD {
            size,
            data: entries,
        });
    }
    Err("cube file has no LUT_1D_SIZE or LUT_3D_SIZE".into())
}

fn sample_one_d(lut: &[[f32; 3]], channel: usize, value: f32) -> f32 {
    if lut.is_empty() {
        return value;
    }
    let max_index = lut.len() - 1;
    let scaled = value.clamp(0.0, 1.0) * max_index as f32;
    let lower = scaled.floor() as usize;
    let upper = (lower + 1).min(max_index);
    let t = scaled - lower as f32;
    lut[lower][channel] + (lut[upper][channel] - lut[lower][channel]) * t
}

fn sample_three_d(data: &[[f32; 3]], size: usize, r: f32, g: f32, b: f32) -> [f32; 3] {
    let max_index = size - 1;
    let scaled_r = r.clamp(0.0, 1.0) * max_index as f32;
    let scaled_g = g.clamp(0.0, 1.0) * max_index as f32;
    let scaled_b = b.clamp(0.0, 1.0) * max_index as f32;
    let r0 = scaled_r.floor() as usize;
    let g0 = scaled_g.floor() as usize;
    let b0 = scaled_b.floor() as usize;
    let r1 = (r0 + 1).min(max_index);
    let g1 = (g0 + 1).min(max_index);
    let b1 = (b0 + 1).min(max_index);
    let fr = scaled_r - r0 as f32;
    let fg = scaled_g - g0 as f32;
    let fb = scaled_b - b0 as f32;
    let at = |r: usize, g: usize, b: usize| data[b + size * (g + size * r)];

    let mut result = [0.0f32; 3];
    for (channel, slot) in result.iter_mut().enumerate() {
        let c000 = at(r0, g0, b0)[channel];
        let c100 = at(r1, g0, b0)[channel];
        let c010 = at(r0, g1, b0)[channel];
        let c110 = at(r1, g1, b0)[channel];
        let c001 = at(r0, g0, b1)[channel];
        let c101 = at(r1, g0, b1)[channel];
        let c011 = at(r0, g1, b1)[channel];
        let c111 = at(r1, g1, b1)[channel];
        let x00 = c000 + (c100 - c000) * fr;
        let x10 = c010 + (c110 - c010) * fr;
        let x01 = c001 + (c101 - c001) * fr;
        let x11 = c011 + (c111 - c011) * fr;
        let y0 = x00 + (x10 - x00) * fg;
        let y1 = x01 + (x11 - x01) * fg;
        *slot = y0 + (y1 - y0) * fb;
    }
    result
}

/// Apply a parsed LUT in display-referred sRGB and blend it with `strength`.
pub fn apply_lut(image: &mut LinearImage, lut: &Lut, strength: f32) {
    if !strength.is_finite() || strength <= 0.0 {
        return;
    }
    let amount = strength.clamp(0.0, 1.0);
    let size = lut.size();
    if size < 2 {
        return;
    }
    for pixel in image.data.chunks_exact_mut(4) {
        let original = [
            linear_to_srgb(pixel[0].clamp(0.0, 1.0)),
            linear_to_srgb(pixel[1].clamp(0.0, 1.0)),
            linear_to_srgb(pixel[2].clamp(0.0, 1.0)),
        ];
        let mapped = match lut {
            Lut::OneD(values) => [
                sample_one_d(values, 0, original[0]),
                sample_one_d(values, 1, original[1]),
                sample_one_d(values, 2, original[2]),
            ],
            Lut::ThreeD { data, size } => {
                sample_three_d(data, *size, original[0], original[1], original[2])
            }
        };
        for channel in 0..3 {
            let blended = original[channel] + (mapped[channel] - original[channel]) * amount;
            pixel[channel] = srgb_to_linear(blended.clamp(0.0, 1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_core::DecodedImage;

    fn identity_1d() -> String {
        let mut text = "LUT_1D_SIZE 3\n".to_string();
        for value in [0.0, 0.5, 1.0] {
            text.push_str(&format!("{value} {value} {value}\n"));
        }
        text
    }

    fn gray_3d() -> String {
        let mut text = "LUT_3D_SIZE 2\n".to_string();
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    let value = (r + g + b) as f32 / 3.0;
                    text.push_str(&format!("{value} {value} {value}\n"));
                }
            }
        }
        text
    }

    #[test]
    fn parses_1d_and_3d_cube_luts() {
        let one_d = parse_cube(identity_1d().as_bytes()).unwrap();
        assert_eq!(one_d.size(), 3);
        let three_d = parse_cube(gray_3d().as_bytes()).unwrap();
        assert_eq!(three_d.size(), 2);
        assert!(matches!(three_d, Lut::ThreeD { size: 2, .. }));
    }

    #[test]
    fn rejects_incomplete_cube() {
        assert!(parse_cube(b"LUT_3D_SIZE 2\n0 0 0\n1 1 1").is_err());
        assert!(parse_cube(b"0 0 0\n1 1 1").is_err());
    }

    #[test]
    fn identity_lut_is_a_noop_and_strength_blends() {
        let mut rgba = Vec::new();
        for _ in 0..4 {
            rgba.extend_from_slice(&[128, 64, 32, 255]);
        }
        let mut image = DecodedImage::new(2, 2, rgba.clone()).unwrap().to_linear();
        let identity = parse_cube(identity_1d().as_bytes()).unwrap();
        apply_lut(&mut image, &identity, 1.0);
        let expected = DecodedImage::new(2, 2, rgba).unwrap().to_linear();
        for (actual, original) in image.data.iter().zip(expected.data.iter()) {
            assert!((actual - original).abs() < 2e-3);
        }
    }
}
