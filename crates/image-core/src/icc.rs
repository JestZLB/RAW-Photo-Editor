//! Minimal ICC support: parse display-RGB matrix/TRC profiles, convert pixels
//! between such a profile and the app working space (linear sRGB / D65), and
//! synthesize profiles for export. Anything else (LUT-based `mft1`/`mft2`,
//! CMYK, abstract profiles) is reported as unsupported instead of guessed at.

use std::collections::HashMap;

use rayon::prelude::*;

const TAG_SIZE: usize = 12;
const BRADFORD: [[f32; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];
const D50_XYZ: [f32; 3] = [0.9642, 1.0, 0.8249];
const D65_XYZ: [f32; 3] = [0.95047, 1.0, 1.08883];
const D65_XY: [f32; 2] = [0.3127, 0.3290];
/// Linear sRGB from D65 XYZ (IEC 61966-2-1).
const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.2404542, -1.5371385, -0.4985314],
    [-0.969266, 1.8760108, 0.041556],
    [0.0556434, -0.2040259, 1.0572252],
];

/// D65 primaries of the working spaces recognised by name.
pub const SRGB_PRIMARIES: [[f32; 2]; 3] = [[0.6400, 0.3300], [0.3000, 0.6000], [0.1500, 0.0600]];
pub const DISPLAY_P3_PRIMARIES: [[f32; 2]; 3] =
    [[0.6800, 0.3200], [0.2650, 0.6900], [0.1500, 0.0600]];
pub const ADOBE_RGB_PRIMARIES: [[f32; 2]; 3] =
    [[0.6400, 0.3300], [0.2150, 0.7150], [0.2800, 0.0700]];

fn tag_key(name: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*name)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

/// ICC `s15Fixed16Number`.
fn read_fixed(bytes: &[u8], offset: usize) -> Option<f32> {
    Some(read_u32(bytes, offset)? as i32 as f32 / 65536.0)
}

fn write_fixed(out: &mut Vec<u8>, value: f32) {
    let scaled = (value.clamp(-32768.0, 32767.99) * 65536.0).round() as i32;
    out.extend_from_slice(&scaled.to_be_bytes());
}

fn mat_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            out[row][col] = (0..3).map(|k| a[row][k] * b[k][col]).sum();
        }
    }
    out
}

fn mat_vec(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn mat_inverse(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ])
}

/// von Kries / Bradford chromatic adaptation from one white point to another.
/// The inverse matrix is derived rather than hardcoded so the two directions
/// really do cancel out; published rounded constants leave ~0.6% residue.
fn adaptation_matrix(from: [f32; 3], to: [f32; 3]) -> [[f32; 3]; 3] {
    let src = mat_vec(BRADFORD, from);
    let dst = mat_vec(BRADFORD, to);
    let ratio = [
        [dst[0] / src[0], 0.0, 0.0],
        [0.0, dst[1] / src[1], 0.0],
        [0.0, 0.0, dst[2] / src[2]],
    ];
    let inverse = mat_inverse(BRADFORD).unwrap_or(BRADFORD);
    mat_mul(inverse, mat_mul(ratio, BRADFORD))
}

/// RGB -> XYZ for a set of chromaticity primaries and a white point.
pub fn rgb_to_xyz_matrix(primaries: [[f32; 2]; 3], white: [f32; 2]) -> Option<[[f32; 3]; 3]> {
    let to_xyz = |p: &[f32; 2]| [p[0] / p[1], 1.0, (1.0 - p[0] - p[1]) / p[1]];
    let r = to_xyz(&primaries[0]);
    let g = to_xyz(&primaries[1]);
    let b = to_xyz(&primaries[2]);
    let p = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let w = to_xyz(&white);
    let s = mat_vec(mat_inverse(p)?, w);
    Some([
        [p[0][0] * s[0], p[0][1] * s[1], p[0][2] * s[2]],
        [p[1][0] * s[0], p[1][1] * s[1], p[1][2] * s[2]],
        [p[2][0] * s[0], p[2][1] * s[1], p[2][2] * s[2]],
    ])
}

fn matrix_close(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3], tolerance: f32) -> bool {
    (0..3).all(|row| (0..3).all(|col| (a[row][col] - b[row][col]).abs() <= tolerance))
}

fn srgb_decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransferCurve {
    Identity,
    Gamma(f32),
    Srgb,
    /// ICC parametric curves 1..4. `linear_below_d` selects the segmented
    /// variants, `offset` adds the `+ c` term.
    Parametric {
        g: f32,
        a: f32,
        b: f32,
        c: f32,
        d: f32,
        f: f32,
        linear_below_d: bool,
        offset: bool,
    },
}

impl TransferCurve {
    fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Identity => x,
            Self::Gamma(g) => x.powf(*g),
            Self::Srgb => srgb_decode(x),
            Self::Parametric {
                g,
                a,
                b,
                c,
                d,
                f,
                linear_below_d,
                offset,
            } => {
                let threshold = if *linear_below_d { *d } else { -b / a };
                if x >= threshold {
                    let base = (a * x + b).max(0.0).powf(*g);
                    if *offset {
                        base + c
                    } else {
                        base
                    }
                } else if *linear_below_d {
                    c * x
                } else if *offset {
                    *c + *f
                } else {
                    0.0
                }
            }
        }
    }

    fn invert(&self, y: f32) -> f32 {
        let y = y.clamp(0.0, 1.0);
        match self {
            Self::Identity => y,
            Self::Gamma(g) => y.powf(1.0 / g),
            Self::Srgb => {
                if y <= 0.0031308 {
                    y * 12.92
                } else {
                    1.055 * y.powf(1.0 / 2.4) - 0.055
                }
            }
            Self::Parametric {
                g,
                a,
                b,
                c,
                d,
                f,
                linear_below_d,
                offset,
            } => {
                let threshold = if *linear_below_d { *d } else { -b / a };
                let breakpoint = if *linear_below_d {
                    c * threshold
                } else if *offset {
                    c + f
                } else {
                    0.0
                };
                if y > breakpoint {
                    let base = if *offset { (y - c).max(0.0) } else { y };
                    ((base.powf(1.0 / g)) - b) / a
                } else if *linear_below_d {
                    y / c
                } else if *offset {
                    (y - f) / c
                } else {
                    0.0
                }
                .clamp(0.0, 1.0)
            }
        }
    }

    fn matches_srgb(&self, tolerance: f32) -> bool {
        (0..=16).all(|step| {
            let x = step as f32 / 16.0;
            (self.apply(x) - srgb_decode(x)).abs() <= tolerance
        })
    }
}

/// A channel tone reproduction curve: analytic, or a table to interpolate.
#[derive(Debug, Clone)]
enum Curve {
    Analytic(TransferCurve),
    Table(Vec<f32>),
}

impl Curve {
    fn apply(&self, x: f32) -> f32 {
        match self {
            Self::Analytic(curve) => curve.apply(x),
            Self::Table(values) => {
                if values.len() < 2 {
                    return x;
                }
                let scaled = x.clamp(0.0, 1.0) * (values.len() - 1) as f32;
                let lower = scaled.floor() as usize;
                let upper = (lower + 1).min(values.len() - 1);
                let t = scaled - lower as f32;
                values[lower] + (values[upper] - values[lower]) * t
            }
        }
    }

    fn invert(&self, y: f32) -> f32 {
        match self {
            Self::Analytic(curve) => curve.invert(y),
            Self::Table(values) => {
                if values.len() < 2 {
                    return y;
                }
                let target = y.clamp(0.0, 1.0);
                let upper = match values.binary_search_by(|value| {
                    value
                        .partial_cmp(&target)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    Ok(index) => index,
                    Err(index) => index.clamp(1, values.len() - 1),
                };
                let lower = upper - 1;
                let span = values[upper] - values[lower];
                let t = if span.abs() < 1e-6 {
                    0.0
                } else {
                    (target - values[lower]) / span
                };
                (lower as f32 + t) / (values.len() - 1) as f32
            }
        }
    }

    fn analytic(&self) -> Option<&TransferCurve> {
        match self {
            Self::Analytic(curve) => Some(curve),
            Self::Table(_) => None,
        }
    }

    /// Recover a pure power law from a table so gamma-only profiles (Adobe RGB)
    /// can still be classified and inverted.
    fn approx_gamma(&self) -> Option<f32> {
        match self {
            Self::Analytic(TransferCurve::Gamma(g)) => Some(*g),
            Self::Analytic(TransferCurve::Identity) => Some(1.0),
            Self::Table(values) if values.len() >= 2 => {
                let mid = values[values.len() / 2];
                if !(0.0..=1.0).contains(&mid) || mid <= 0.0 || mid >= 1.0 {
                    return None;
                }
                let gamma = mid.ln() / 0.5f32.ln();
                (1..=8)
                    .all(|step| {
                        let x = step as f32 / 8.0;
                        (self.apply(x) - x.powf(gamma)).abs() < 0.01
                    })
                    .then_some(gamma)
            }
            _ => None,
        }
    }

    fn parse(bytes: &[u8], offset: usize, length: usize) -> Option<Self> {
        let data = bytes.get(offset..offset + length)?;
        match read_u32(data, 0)? {
            k if k == tag_key(b"curv") => {
                let count = read_u32(data, 8)? as usize;
                if count == 0 {
                    return Some(Self::Analytic(TransferCurve::Identity));
                }
                if count == 1 {
                    let gamma = read_u16(data, 12)? as f32 / 256.0;
                    return Some(Self::Analytic(TransferCurve::Gamma(gamma.max(0.01))));
                }
                let values: Vec<f32> = (0..count)
                    .filter_map(|index| read_u16(data, 12 + index * 2).map(|v| v as f32 / 65535.0))
                    .collect();
                if values.len() != count {
                    return None;
                }
                Some(Self::Table(values))
            }
            k if k == tag_key(b"para") => {
                let kind = read_u16(data, 8)?;
                let param = |index: usize| read_fixed(data, 12 + index * 4);
                let g = param(0)?;
                let curve = match kind {
                    0 => TransferCurve::Gamma(g.max(0.01)),
                    1 => TransferCurve::Parametric {
                        g,
                        a: param(1)?,
                        b: param(2)?,
                        c: 0.0,
                        d: 0.0,
                        f: 0.0,
                        linear_below_d: false,
                        offset: false,
                    },
                    2 => TransferCurve::Parametric {
                        g,
                        a: param(1)?,
                        b: param(2)?,
                        c: param(3)?,
                        d: 0.0,
                        f: 0.0,
                        linear_below_d: false,
                        offset: true,
                    },
                    3 => TransferCurve::Parametric {
                        g,
                        a: param(1)?,
                        b: param(2)?,
                        c: param(3)?,
                        d: param(4)?,
                        f: 0.0,
                        linear_below_d: true,
                        offset: false,
                    },
                    4 => TransferCurve::Parametric {
                        g,
                        a: param(1)?,
                        b: param(2)?,
                        c: param(3)?,
                        d: param(4)?,
                        f: param(5)?,
                        linear_below_d: true,
                        offset: true,
                    },
                    _ => return None,
                };
                Some(Self::Analytic(curve))
            }
            k if k == tag_key(b"mntr") => {
                let gamma = *data.get(8)? as f32 / 256.0;
                Some(Self::Analytic(if gamma.abs() < 0.01 {
                    TransferCurve::Identity
                } else {
                    TransferCurve::Gamma(gamma.max(0.01))
                }))
            }
            _ => None,
        }
    }
}

/// How a source profile relates to the app working space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamutLabel {
    Srgb,
    DisplayP3,
    AdobeRgb,
    OtherRgb,
    NonRgb,
    Unsupported,
}

impl GamutLabel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::DisplayP3 => "Display P3",
            Self::AdobeRgb => "Adobe RGB (1998)",
            Self::OtherRgb => "其他 RGB",
            Self::NonRgb => "非 RGB",
            Self::Unsupported => "未识别",
        }
    }
}

#[derive(Debug, Clone)]
struct RgbConverter {
    /// Encoded value -> linear value, per channel.
    curves: [Curve; 3],
    /// Profile RGB (after the TRC) -> D50 PCS XYZ.
    to_pcs: [[f32; 3]; 3],
    /// D50 PCS XYZ -> linear sRGB (D65).
    pcs_to_working: [[f32; 3]; 3],
    /// Inverse of `pcs_to_working`, resolved once.
    working_to_pcs: [[f32; 3]; 3],
}

/// An embedded ICC profile plus the decisions derived from it.
#[derive(Debug, Clone)]
pub struct IccProfile {
    pub bytes: Vec<u8>,
    pub description: String,
    pub gamut: GamutLabel,
    /// The profile is close enough to sRGB that pixels pass through unchanged.
    pub matches_working_space: bool,
    converter: Option<RgbConverter>,
}

impl IccProfile {
    /// Parse a profile. Anything malformed yields `None` so the caller keeps
    /// the documented "assume sRGB" behaviour instead of failing the import.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 132 || bytes.get(36..40) != Some(b"acsp") {
            return None;
        }
        let declared = read_u32(bytes, 0)? as usize;
        if declared > bytes.len() {
            return None;
        }
        let class: [u8; 4] = bytes.get(12..16)?.try_into().ok()?;
        let space: [u8; 4] = bytes.get(16..20)?.try_into().ok()?;

        let tag_count = read_u32(bytes, 128)? as usize;
        if tag_count > 200 || 132 + tag_count * TAG_SIZE > bytes.len() {
            return None;
        }
        let mut tags = HashMap::new();
        for index in 0..tag_count {
            let entry = 132 + index * TAG_SIZE;
            let key = read_u32(bytes, entry)?;
            let offset = read_u32(bytes, entry + 4)? as usize;
            let length = read_u32(bytes, entry + 8)? as usize;
            if length < 8 || offset + length > bytes.len() {
                continue;
            }
            tags.insert(key, (offset, length));
        }

        let description = tags
            .get(&tag_key(b"desc"))
            .and_then(|(offset, length)| read_description(bytes, *offset, *length))
            .or_else(|| {
                tags.get(&tag_key(b"dscm"))
                    .and_then(|(offset, length)| read_description(bytes, *offset, *length))
            })
            .unwrap_or_default();

        let converter = if space == *b"RGB " && class == *b"mntr" {
            build_converter(bytes, &tags)
        } else {
            None
        };

        let (gamut, matches_working_space) = match &converter {
            Some(converter) => classify(converter),
            None if space == *b"RGB " => (GamutLabel::Unsupported, false),
            None => (GamutLabel::NonRgb, false),
        };

        Some(Self {
            bytes: bytes.to_vec(),
            description,
            gamut,
            matches_working_space,
            converter,
        })
    }

    pub fn can_convert(&self) -> bool {
        self.converter.is_some()
    }

    /// Convert 8-bit encoded pixels from the profile into working sRGB pixels.
    pub fn convert_pixels(&self, rgba: &mut [u8]) {
        // An sRGB-equivalent profile must stay a bitwise no-op, and a profile
        // we cannot invert must never silently guess.
        if self.matches_working_space {
            return;
        }
        let Some(converter) = &self.converter else {
            return;
        };
        // The input is 8-bit, so the per-channel TRC collapses into a LUT.
        let mut luts = [[0f32; 256]; 3];
        for (channel, lut) in luts.iter_mut().enumerate() {
            for (value, slot) in lut.iter_mut().enumerate() {
                *slot = converter.curves[channel].apply(value as f32 / 255.0);
            }
        }
        rgba.par_chunks_exact_mut(4).for_each(|pixel| {
            let linear = [
                luts[0][pixel[0] as usize],
                luts[1][pixel[1] as usize],
                luts[2][pixel[2] as usize],
            ];
            let xyz = mat_vec(converter.to_pcs, linear);
            let working = mat_vec(converter.pcs_to_working, xyz);
            for channel in 0..3 {
                pixel[channel] =
                    (srgb_encode(working[channel].clamp(0.0, 1.0)) * 255.0).round() as u8;
            }
        });
    }

    /// 16-bit variant of [`Self::convert_pixels`]; same math, wider scale.
    pub fn convert_pixels_16(&self, rgba: &mut [u16]) {
        self.remap_16(rgba, true);
    }

    /// 16-bit variant of [`Self::convert_pixels_from_working`].
    pub fn convert_pixels_from_working_16(&self, rgba: &mut [u16]) {
        self.remap_16(rgba, false);
    }

    fn remap_16(&self, rgba: &mut [u16], into_working: bool) {
        let Some(converter) = &self.converter else {
            return;
        };
        if self.matches_working_space {
            return;
        }
        let to_pcs_inverse = mat_inverse(converter.to_pcs).unwrap_or(converter.to_pcs);
        rgba.par_chunks_exact_mut(4).for_each(|pixel| {
            if into_working {
                let linear = [
                    converter.curves[0].apply(pixel[0] as f32 / 65535.0),
                    converter.curves[1].apply(pixel[1] as f32 / 65535.0),
                    converter.curves[2].apply(pixel[2] as f32 / 65535.0),
                ];
                let xyz = mat_vec(converter.to_pcs, linear);
                let working = mat_vec(converter.pcs_to_working, xyz);
                for channel in 0..3 {
                    pixel[channel] = crate::linear_to_srgb_u16(working[channel]);
                }
            } else {
                let working = [
                    crate::srgb_u16_to_linear(pixel[0]),
                    crate::srgb_u16_to_linear(pixel[1]),
                    crate::srgb_u16_to_linear(pixel[2]),
                ];
                let xyz = mat_vec(converter.working_to_pcs, working);
                let linear = mat_vec(to_pcs_inverse, xyz);
                for channel in 0..3 {
                    let encoded = converter.curves[channel].invert(linear[channel].clamp(0.0, 1.0));
                    pixel[channel] = (encoded.clamp(0.0, 1.0) * 65535.0).round() as u16;
                }
            }
        });
    }

    /// Convert working sRGB pixels into this profile's space (export side).
    pub fn convert_pixels_from_working(&self, rgba: &mut [u8]) {
        if self.matches_working_space {
            return;
        }
        let Some(converter) = &self.converter else {
            return;
        };
        rgba.par_chunks_exact_mut(4).for_each(|pixel| {
            let working = [
                srgb_decode(pixel[0] as f32 / 255.0),
                srgb_decode(pixel[1] as f32 / 255.0),
                srgb_decode(pixel[2] as f32 / 255.0),
            ];
            let xyz = mat_vec(converter.working_to_pcs, working);
            let linear = mat_vec(
                mat_inverse(converter.to_pcs).unwrap_or(converter.to_pcs),
                xyz,
            );
            for channel in 0..3 {
                let encoded = converter.curves[channel].invert(linear[channel].clamp(0.0, 1.0));
                pixel[channel] = (encoded.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
    }
}

fn build_converter(bytes: &[u8], tags: &HashMap<u32, (usize, usize)>) -> Option<RgbConverter> {
    let column = |name: &[u8; 4]| {
        let (offset, _) = tags.get(&tag_key(name))?;
        // XYZ tags carry an 8-byte type + reserved header before the values.
        let base = offset + 8;
        Some([
            read_fixed(bytes, base)?,
            read_fixed(bytes, base + 4)?,
            read_fixed(bytes, base + 8)?,
        ])
    };
    let r = column(b"rXYZ")?;
    let g = column(b"gXYZ")?;
    let b = column(b"bXYZ")?;
    let to_pcs = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];

    let read_curve = |name: &[u8; 4]| {
        tags.get(&tag_key(name))
            .and_then(|(offset, length)| Curve::parse(bytes, *offset, *length))
    };
    let red = read_curve(b"rTRC")?;
    let curves = [
        red.clone(),
        read_curve(b"gTRC").unwrap_or_else(|| red.clone()),
        read_curve(b"bTRC").unwrap_or_else(|| red.clone()),
    ];

    // PCS(D50) XYZ -> linear sRGB(D65). The profile matrix itself is applied
    // separately, so this must not fold `to_pcs` in again.
    let pcs_to_working = mat_mul(XYZ_TO_SRGB, adaptation_matrix(D50_XYZ, D65_XYZ));
    let working_to_pcs = mat_inverse(pcs_to_working)?;
    Some(RgbConverter {
        curves,
        to_pcs,
        pcs_to_working,
        working_to_pcs,
    })
}

fn classify(converter: &RgbConverter) -> (GamutLabel, bool) {
    // Compare in D65: undo the profile's adaptation into the PCS.
    let adapted = mat_mul(adaptation_matrix(D50_XYZ, D65_XYZ), converter.to_pcs);
    let Some(srgb) = rgb_to_xyz_matrix(SRGB_PRIMARIES, D65_XY) else {
        return (GamutLabel::Unsupported, false);
    };
    let p3 = rgb_to_xyz_matrix(DISPLAY_P3_PRIMARIES, D65_XY);
    let adobe = rgb_to_xyz_matrix(ADOBE_RGB_PRIMARIES, D65_XY);
    let srgb_curves = converter
        .curves
        .iter()
        .all(|curve| curve.analytic().is_some_and(|c| c.matches_srgb(0.02)));
    let gamma_curves = converter
        .curves
        .iter()
        .all(|curve| curve.approx_gamma().is_some_and(|g| (g - 2.2).abs() < 0.08));
    let matches_working_space = matrix_close(&adapted, &srgb, 0.02) && srgb_curves;
    let gamut = if matches_working_space {
        GamutLabel::Srgb
    } else if p3.is_some_and(|m| matrix_close(&adapted, &m, 0.02)) && srgb_curves {
        GamutLabel::DisplayP3
    } else if adobe.is_some_and(|m| matrix_close(&adapted, &m, 0.02)) && gamma_curves {
        GamutLabel::AdobeRgb
    } else {
        GamutLabel::OtherRgb
    };
    (gamut, matches_working_space)
}

fn read_description(bytes: &[u8], offset: usize, length: usize) -> Option<String> {
    let data = bytes.get(offset..offset + length)?;
    match read_u32(data, 0)? {
        k if k == tag_key(b"desc") || k == tag_key(b"dscm") => {
            let count = read_u32(data, 8)? as usize;
            let raw = data.get(12..12 + count.min(data.len().saturating_sub(12)))?;
            Some(sanitize_text(&String::from_utf8_lossy(raw)))
        }
        k if k == tag_key(b"mluc") => {
            let count = read_u32(data, 8)? as usize;
            let mut best: Option<String> = None;
            for index in 0..count {
                let record = 12 + index * 12;
                let encoding = read_u32(data, record)?;
                let size = read_u32(data, record + 4)? as usize;
                let string_offset = read_u32(data, record + 8)? as usize;
                let raw = data.get(string_offset..string_offset + size)?;
                let text = match encoding {
                    0 | 5 => String::from_utf8_lossy(raw).to_string(),
                    3 => String::from_utf16_lossy(
                        &raw.chunks_exact(2)
                            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                            .collect::<Vec<u16>>(),
                    ),
                    _ => continue,
                };
                let text = sanitize_text(&text);
                if best.is_none() || encoding == 0 {
                    best = Some(text);
                }
            }
            best
        }
        _ => None,
    }
}

fn sanitize_text(input: &str) -> String {
    input
        .trim_matches('\0')
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect()
}

/// Transfer function used by a synthesized profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileTrc {
    /// The sRGB curve (ICC parametric type 3).
    Srgb,
    /// A pure power law, e.g. Adobe RGB's 2.2.
    Gamma(f32),
}

/// Build a display-RGB ICC profile (v2.1, matrix + TRC) for `primaries`.
/// The result is valid ICC used for exports and tests alike.
pub fn build_rgb_profile(name: &str, primaries: [[f32; 2]; 3], trc: ProfileTrc) -> Option<Vec<u8>> {
    let d65 = rgb_to_xyz_matrix(primaries, D65_XY)?;
    let to_pcs = mat_mul(adaptation_matrix(D65_XYZ, D50_XYZ), d65);

    let mut description = Vec::new();
    description.extend_from_slice(b"desc");
    description.extend_from_slice(&0u32.to_be_bytes());
    let text = sanitize_text(name);
    description.extend_from_slice(&(text.len() as u32 + 1).to_be_bytes());
    description.extend_from_slice(text.as_bytes());
    description.push(0);
    while description.len() % 4 != 0 {
        description.push(0);
    }

    let mut curve_tag = Vec::new();
    match trc {
        ProfileTrc::Srgb => {
            curve_tag.extend_from_slice(b"para");
            curve_tag.extend_from_slice(&0u32.to_be_bytes());
            curve_tag.extend_from_slice(&3u16.to_be_bytes());
            curve_tag.extend_from_slice(&0u16.to_be_bytes());
            for value in [2.4f32, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, -0.055] {
                write_fixed(&mut curve_tag, value);
            }
        }
        ProfileTrc::Gamma(gamma) => {
            curve_tag.extend_from_slice(b"curv");
            curve_tag.extend_from_slice(&0u32.to_be_bytes());
            curve_tag.extend_from_slice(&1u32.to_be_bytes());
            curve_tag.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
            curve_tag.extend_from_slice(&0u16.to_be_bytes());
        }
    }

    let xyz_tag = |value: [f32; 3]| {
        let mut out = Vec::new();
        out.extend_from_slice(b"XYZ ");
        out.extend_from_slice(&0u32.to_be_bytes());
        for component in value {
            write_fixed(&mut out, component);
        }
        out
    };

    let pad = |mut payload: Vec<u8>| {
        let padding = (4 - payload.len() % 4) % 4;
        payload.resize(payload.len() + padding, 0);
        payload
    };
    let curve_tag = pad(curve_tag);
    let payloads: [(&[u8; 4], Vec<u8>); 8] = [
        (b"desc", description),
        (b"wtpt", xyz_tag(D50_XYZ)),
        (b"rTRC", curve_tag.clone()),
        (b"gTRC", curve_tag.clone()),
        (b"bTRC", curve_tag.clone()),
        (b"rXYZ", xyz_tag([to_pcs[0][0], to_pcs[1][0], to_pcs[2][0]])),
        (b"gXYZ", xyz_tag([to_pcs[0][1], to_pcs[1][1], to_pcs[2][1]])),
        (b"bXYZ", xyz_tag([to_pcs[0][2], to_pcs[1][2], to_pcs[2][2]])),
    ];

    let header_size = 128 + 4 + payloads.len() * TAG_SIZE;
    let mut table = Vec::new();
    let mut body = Vec::new();
    let mut offset = header_size;
    for (key, payload) in &payloads {
        table.extend_from_slice(*key);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        body.extend_from_slice(payload);
        offset += payload.len();
        let padding = (4 - offset % 4) % 4;
        body.resize(body.len() + padding, 0);
        offset += padding;
    }

    let mut out = vec![0u8; 128];
    let total = header_size + body.len();
    out[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    out[4..8].copy_from_slice(b"Luma");
    out[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(b"RGB ");
    out[20..24].copy_from_slice(b"XYZ ");
    out[24..28].copy_from_slice(b"acgp");
    out[28..32].copy_from_slice(b"none");
    out[36..40].copy_from_slice(b"acsp");
    out[40..44].copy_from_slice(b"APPL");
    out[64..68].copy_from_slice(&[0x00, 0x00, 0xF6, 0xD6]);
    out[68..72].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    out[72..76].copy_from_slice(&[0x00, 0x00, 0xD2, 0xB4]);
    out.extend_from_slice(&(payloads.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_for(primaries: [[f32; 2]; 3], name: &str, trc: ProfileTrc) -> IccProfile {
        let bytes = build_rgb_profile(name, primaries, trc).unwrap();
        IccProfile::parse(&bytes).expect("profile parses")
    }

    #[test]
    fn srgb_profile_is_detected_and_is_a_no_op() {
        let profile = profile_for(SRGB_PRIMARIES, "sRGB Test", ProfileTrc::Srgb);
        assert_eq!(profile.gamut, GamutLabel::Srgb);
        assert!(profile.matches_working_space);
        assert_eq!(profile.description, "sRGB Test");

        let mut pixels = vec![18, 128, 255, 255];
        profile.convert_pixels(&mut pixels);
        assert_eq!(pixels, vec![18, 128, 255, 255]);
    }

    #[test]
    fn display_p3_profile_is_detected_and_widens_reds() {
        let profile = profile_for(DISPLAY_P3_PRIMARIES, "Display P3", ProfileTrc::Srgb);
        assert_eq!(profile.gamut, GamutLabel::DisplayP3);
        assert!(!profile.matches_working_space);

        // Import direction: a P3 red is outside the sRGB gamut, so it clips to
        // the sRGB red primary instead of staying in the wider space.
        let mut saturated = vec![255, 0, 0, 255];
        profile.convert_pixels(&mut saturated);
        assert_eq!(&saturated[..3], &[255, 0, 0]);

        // Export direction: sRGB primaries sit *inside* P3, so the same red has
        // to move away from the P3 corner and pick up its green/blue primaries.
        let mut working_red = vec![255, 0, 0, 255];
        profile.convert_pixels_from_working(&mut working_red);
        assert!(
            working_red[0] < 250 && working_red[0] > 200,
            "{working_red:?}"
        );
        assert!(working_red[1] > 40, "{working_red:?}");
        assert!(working_red[2] > 0, "{working_red:?}");

        let mut neutral = vec![128, 128, 128, 255];
        profile.convert_pixels(&mut neutral);
        assert!((neutral[0] as i16 - neutral[2] as i16).abs() <= 2);
        assert!((neutral[0] as i16 - 128).abs() <= 3);
    }

    #[test]
    fn white_point_survives_conversion() {
        let profile = profile_for(DISPLAY_P3_PRIMARIES, "Display P3", ProfileTrc::Srgb);
        let mut white = vec![255, 255, 255, 255];
        profile.convert_pixels(&mut white);
        assert_eq!(&white[..3], &[255, 255, 255]);
    }

    #[test]
    fn conversion_round_trip_back_to_the_source_profile_is_stable() {
        let profile = profile_for(DISPLAY_P3_PRIMARIES, "Display P3", ProfileTrc::Srgb);
        let mut pixels = vec![210, 90, 40, 255];
        let expected = pixels.clone();
        profile.convert_pixels(&mut pixels);
        profile.convert_pixels_from_working(&mut pixels);
        for (got, want) in pixels.iter().zip(expected.iter()) {
            assert!((*got as i16 - *want as i16).abs() <= 2, "{got} vs {want}");
        }
    }

    #[test]
    fn adobe_rgb_is_classified_by_its_gamma_and_wider_green() {
        let profile = profile_for(ADOBE_RGB_PRIMARIES, "Adobe RGB", ProfileTrc::Gamma(2.2));
        assert_eq!(profile.gamut, GamutLabel::AdobeRgb);
        assert!(!profile.matches_working_space);

        // Adobe's green primary is outside sRGB: importing clips it back down.
        let mut green = vec![0, 255, 0, 255];
        profile.convert_pixels(&mut green);
        assert!(green[1] > 200);
        assert_eq!(green[0], 0);

        // Exporting an sRGB green into Adobe RGB pulls it off the Adobe corner.
        let mut working_green = vec![0, 255, 0, 255];
        profile.convert_pixels_from_working(&mut working_green);
        assert!(
            working_green[1] < 250 && working_green[1] > 200,
            "{working_green:?}"
        );
        assert!(working_green[0] > 0, "{working_green:?}");
    }

    #[test]
    fn unknown_rgb_primaries_are_reported_but_still_converted() {
        let odd = [[0.6600, 0.3100], [0.2500, 0.7000], [0.1400, 0.0500]];
        let profile = profile_for(odd, "Odd Space", ProfileTrc::Srgb);
        assert_eq!(profile.gamut, GamutLabel::OtherRgb);
        assert!(profile.can_convert());
        let mut pixels = vec![10, 20, 30, 255];
        profile.convert_pixels(&mut pixels);
        assert_ne!(pixels, vec![10, 20, 30, 255]);
    }

    #[test]
    fn garbage_non_rgb_and_truncated_profiles_are_rejected_or_skipped() {
        assert!(IccProfile::parse(b"not a profile").is_none());
        assert!(IccProfile::parse(&[0u8; 8]).is_none());

        let mut bytes = build_rgb_profile("Odd", SRGB_PRIMARIES, ProfileTrc::Srgb).unwrap();
        bytes[16..20].copy_from_slice(b"CMYK");
        let profile = IccProfile::parse(&bytes).unwrap();
        assert_eq!(profile.gamut, GamutLabel::NonRgb);
        assert!(!profile.can_convert());
        let mut pixels = vec![10, 20, 30, 255];
        profile.convert_pixels(&mut pixels);
        assert_eq!(pixels, vec![10, 20, 30, 255]);

        assert!(IccProfile::parse(&bytes[..bytes.len() / 2]).is_none());
    }

    #[test]
    fn curve_math_matches_known_transfer_functions() {
        let srgb = TransferCurve::Srgb;
        assert!((srgb.apply(0.5) - 0.2140).abs() < 0.001);
        assert!((srgb.invert(0.2140) - 0.5).abs() < 0.001);
        let gamma = TransferCurve::Gamma(2.2);
        assert!((gamma.apply(0.5) - 0.2176).abs() < 0.001);
        assert!((gamma.invert(0.2176) - 0.5).abs() < 0.001);
        assert!(srgb.matches_srgb(1e-6));
        assert!(!gamma.matches_srgb(0.001));

        let values: Vec<f32> = (0..=255)
            .map(|index| (index as f32 / 255.0).powf(2.2))
            .collect();
        let table = Curve::Table(values);
        assert!((table.apply(0.5) - 0.2176).abs() < 0.01);
        assert!((table.invert(0.2176) - 0.5).abs() < 0.01);
        assert!((table.approx_gamma().unwrap() - 2.2).abs() < 0.05);
    }

    #[test]
    fn synthesized_profile_declares_its_own_size() {
        let bytes = build_rgb_profile("Sizing", DISPLAY_P3_PRIMARIES, ProfileTrc::Srgb).unwrap();
        let declared = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
        assert_eq!(declared, bytes.len());
        assert_eq!(&bytes[36..40], b"acsp");
    }
}
