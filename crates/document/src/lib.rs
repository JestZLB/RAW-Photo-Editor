//! Non-destructive project document model and undo/redo session.
//!
//! Snapshots stored in the undo stack contain only serialized metadata (layer
//! parameters, layer order, source reference) — never pixel buffers.

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 18;
pub const WORKING_COLOR_SPACE: &str = "linear-srgb";

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn default_curve_y() -> [f32; 5] {
    [0.0, 0.25, 0.5, 0.75, 1.0]
}

fn clamp_finite(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub working_color_space: String,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            working_color_space: WORKING_COLOR_SPACE.to_string(),
        }
    }
}

/// Crop rectangle in original source pixels. Stored in the document so the
/// original file and all layer parameters stay untouched.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Clockwise straighten angle in degrees. The full frame is rotated before
    /// `x`/`y`/`width`/`height` are applied, so they live in the rotated
    /// bounding box, not in the source frame.
    pub angle: f32,
}

impl CropRect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
            angle: 0.0,
        }
    }

    /// Size of the bounding box that holds the frame rotated by `angle`.
    pub fn rotated_frame(width: u32, height: u32, angle: f32) -> (u32, u32) {
        let radians = angle.to_radians();
        let (sin, cos) = radians.sin_cos();
        let (sin, cos) = (sin.abs(), cos.abs());
        // The epsilon keeps exact right angles from rounding up by one pixel.
        let frame_width = (width as f32 * cos + height as f32 * sin - 1e-4).ceil();
        let frame_height = (width as f32 * sin + height as f32 * cos - 1e-4).ceil();
        ((frame_width as u32).max(1), (frame_height as u32).max(1))
    }

    pub fn clamped(self, canvas_width: u32, canvas_height: u32) -> Self {
        if canvas_width == 0 || canvas_height == 0 {
            return Self::default();
        }
        let angle = clamp_finite(self.angle, -45.0, 45.0, 0.0);
        let (frame_width, frame_height) = Self::rotated_frame(canvas_width, canvas_height, angle);
        let width = self.width.clamp(1, frame_width);
        let height = self.height.clamp(1, frame_height);
        Self {
            x: self.x.min(frame_width - width),
            y: self.y.min(frame_height - height),
            width,
            height,
            angle,
        }
    }

    pub fn is_full(self, canvas_width: u32, canvas_height: u32) -> bool {
        self.angle.abs() < 0.001
            && self.x == 0
            && self.y == 0
            && self.width == canvas_width
            && self.height == canvas_height
    }

    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAsset {
    pub asset_id: Uuid,
    pub display_name: String,
    pub width: u32,
    pub height: u32,
    /// Logical format label: `jpeg`, `png`, `tiff`, `webp`.
    pub format: String,
    /// Original file on disk. Never copied/overwritten by `import_photo`.
    pub original_path: String,
    pub file_bytes: u64,
    /// Colour space the file declared, e.g. `sRGB`, `Display P3`. Documents
    /// written before schema 8 read back as `sRGB`, which is what they were
    /// treated as anyway.
    #[serde(default = "default_color_space")]
    pub color_space: String,
    /// Embedded profile that pixels were converted *from*. Present only when
    /// the import actually remapped the image, which lets an export write the
    /// file back in the source space.
    #[serde(default)]
    pub icc_asset_id: Option<Uuid>,
}

fn default_color_space() -> String {
    "sRGB".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerKind {
    Base,
    Portrait,
    Color,
    AiResult,
    Retouch,
}

/// How a retouch stamp fills its disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RetouchMode {
    /// Copy pixels from an explicit sample point.
    #[default]
    Clone,
    /// Copy mirrored pixels and match their colour to the ring around the spot.
    Heal,
}

/// One circular retouch stamp in normalized source coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetouchSpot {
    pub mode: RetouchMode,
    pub x: f32,
    pub y: f32,
    /// Relative to the shorter source side, like the mask brush.
    pub radius: f32,
    pub source_x: Option<f32>,
    pub source_y: Option<f32>,
    /// 0 = fully feathered edge, 1 = hard edge.
    pub hardness: f32,
    pub opacity: f32,
}

impl Default for RetouchSpot {
    fn default() -> Self {
        Self {
            mode: RetouchMode::Clone,
            x: 0.5,
            y: 0.5,
            radius: 0.02,
            source_x: None,
            source_y: None,
            hardness: 0.4,
            opacity: 1.0,
        }
    }
}

impl RetouchSpot {
    pub fn sanitized(&self) -> Self {
        Self {
            mode: self.mode,
            x: clamp_finite(self.x, 0.0, 1.0, 0.5),
            y: clamp_finite(self.y, 0.0, 1.0, 0.5),
            radius: clamp_finite(self.radius, 0.002, 0.25, 0.02),
            source_x: self
                .source_x
                .map(|value| clamp_finite(value, 0.0, 1.0, 0.5)),
            source_y: self
                .source_y
                .map(|value| clamp_finite(value, 0.0, 1.0, 0.5)),
            hardness: clamp_finite(self.hardness, 0.0, 1.0, 0.4),
            opacity: clamp_finite(self.opacity, 0.0, 1.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiquifyMode {
    /// Drag pixels along the pointer path.
    Push,
    /// Pull pixels toward the pointer, compressing the area.
    Pinch,
    /// Push pixels away from the pointer, enlarging the area.
    Bulge,
}

/// One non-destructive liquify stroke. Coordinates are normalized to the
/// uncropped source; radius is a fraction of its shortest side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiquifyStroke {
    pub points: Vec<MaskPoint>,
    pub radius: f32,
    /// 0..1, scaled by the layer opacity at render time.
    pub strength: f32,
    pub mode: LiquifyMode,
}

impl Default for LiquifyStroke {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            radius: 0.05,
            strength: 0.5,
            mode: LiquifyMode::Push,
        }
    }
}

impl LiquifyStroke {
    pub fn sanitized(&self) -> Self {
        let mut points = self.points.clone();
        points.truncate(1024);
        for point in &mut points {
            point.x = clamp_finite(point.x, 0.0, 1.0, 0.5);
            point.y = clamp_finite(point.y, 0.0, 1.0, 0.5);
        }
        Self {
            points,
            radius: clamp_finite(self.radius, 0.002, 0.25, 0.05),
            strength: clamp_finite(self.strength, 0.0, 1.0, 0.5),
            mode: self.mode,
        }
    }
}

/// Grid protection resolution. Cells cover the full uncropped frame so a crop
/// or straighten never changes which cells the user protected.
pub const FREEZE_GRID_COLS: usize = 16;
pub const FREEZE_GRID_ROWS: usize = 16;

/// Non-destructive clone/heal stamps and liquify strokes, applied in order to
/// the composite below.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RetouchParams {
    pub spots: Vec<RetouchSpot>,
    #[serde(default)]
    pub warp: Vec<LiquifyStroke>,
    /// Painted protection for the warp pass; coverage 1 means frozen.
    #[serde(default)]
    pub freeze: Vec<MaskStroke>,
    /// Row-major grid protection: `true` keeps that cell completely still.
    #[serde(default)]
    pub freeze_grid: Vec<bool>,
}

/// Color adjustment parameters. Ranges mirror the IPC contract.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ColorParams {
    /// -3..3 EV
    pub exposure: f32,
    /// -100..100
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub clarity: f32,
    /// 0..100
    pub sharpness: f32,
    /// -180..180 degrees, display-referred HSL hue shift.
    pub hue: f32,
    /// -100..100, HSL saturation multiplier.
    pub hsl_saturation: f32,
    /// -100..100, HSL lightness adjustment.
    pub hsl_lightness: f32,
    /// Five fixed input positions for the RGB master curve.
    pub curve_x: [f32; 5],
    /// Five output positions for the RGB master curve.
    pub curve_y: [f32; 5],
    pub levels_in_black: f32,
    pub levels_in_white: f32,
    pub levels_gamma: f32,
    pub levels_out_black: f32,
    pub levels_out_white: f32,
    /// Imported `.cube` LUT asset. `None` means no look transform is applied.
    pub lut_asset_id: Option<Uuid>,
    /// 0..1 blend between the un-graded image and the LUT result.
    pub lut_strength: f32,
    /// 0..100 luminance noise reduction.
    pub noise_luma: f32,
    /// 0..100 chroma (colour) noise reduction.
    pub noise_chroma: f32,
    /// -100..100 vignette; negative values brighten the corners.
    pub vignette: f32,
    /// 0..100 film grain.
    pub grain: f32,
    /// 0..100 haze removal.
    pub dehaze: f32,
    /// 0..100 purple/green fringe correction.
    pub chromatic: f32,
}

impl Default for ColorParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            temperature: 0.0,
            tint: 0.0,
            saturation: 0.0,
            vibrance: 0.0,
            clarity: 0.0,
            sharpness: 0.0,
            hue: 0.0,
            hsl_saturation: 0.0,
            hsl_lightness: 0.0,
            curve_x: [0.0, 0.25, 0.5, 0.75, 1.0],
            curve_y: [0.0, 0.25, 0.5, 0.75, 1.0],
            levels_in_black: 0.0,
            levels_in_white: 1.0,
            levels_gamma: 1.0,
            levels_out_black: 0.0,
            levels_out_white: 1.0,
            lut_asset_id: None,
            lut_strength: 1.0,
            noise_luma: 0.0,
            noise_chroma: 0.0,
            vignette: 0.0,
            grain: 0.0,
            dehaze: 0.0,
            chromatic: 0.0,
        }
    }
}

impl ColorParams {
    /// Clamp every field into its documented range. Non-finite values fall back
    /// to zero so malformed model/tool JSON can never reach the render graph.
    pub fn sanitized(&self) -> Self {
        let mut out = Self {
            exposure: clamp_finite(self.exposure, -3.0, 3.0, 0.0),
            contrast: clamp_finite(self.contrast, -100.0, 100.0, 0.0),
            highlights: clamp_finite(self.highlights, -100.0, 100.0, 0.0),
            shadows: clamp_finite(self.shadows, -100.0, 100.0, 0.0),
            whites: clamp_finite(self.whites, -100.0, 100.0, 0.0),
            blacks: clamp_finite(self.blacks, -100.0, 100.0, 0.0),
            temperature: clamp_finite(self.temperature, -100.0, 100.0, 0.0),
            tint: clamp_finite(self.tint, -100.0, 100.0, 0.0),
            saturation: clamp_finite(self.saturation, -100.0, 100.0, 0.0),
            vibrance: clamp_finite(self.vibrance, -100.0, 100.0, 0.0),
            clarity: clamp_finite(self.clarity, -100.0, 100.0, 0.0),
            sharpness: clamp_finite(self.sharpness, 0.0, 100.0, 0.0),
            noise_luma: clamp_finite(self.noise_luma, 0.0, 100.0, 0.0),
            noise_chroma: clamp_finite(self.noise_chroma, 0.0, 100.0, 0.0),
            vignette: clamp_finite(self.vignette, -100.0, 100.0, 0.0),
            grain: clamp_finite(self.grain, 0.0, 100.0, 0.0),
            dehaze: clamp_finite(self.dehaze, 0.0, 100.0, 0.0),
            chromatic: clamp_finite(self.chromatic, 0.0, 100.0, 0.0),
            hue: clamp_finite(self.hue, -180.0, 180.0, 0.0),
            hsl_saturation: clamp_finite(self.hsl_saturation, -100.0, 100.0, 0.0),
            hsl_lightness: clamp_finite(self.hsl_lightness, -100.0, 100.0, 0.0),
            curve_x: self.curve_x.map(|value| clamp_finite(value, 0.0, 1.0, 0.5)),
            curve_y: self.curve_y.map(|value| clamp_finite(value, 0.0, 1.0, 0.5)),
            levels_in_black: clamp_finite(self.levels_in_black, 0.0, 0.9, 0.0),
            levels_in_white: clamp_finite(self.levels_in_white, 0.1, 1.0, 1.0),
            levels_gamma: clamp_finite(self.levels_gamma, 0.1, 10.0, 1.0),
            levels_out_black: clamp_finite(self.levels_out_black, 0.0, 0.9, 0.0),
            levels_out_white: clamp_finite(self.levels_out_white, 0.1, 1.0, 1.0),
            lut_asset_id: self.lut_asset_id,
            lut_strength: clamp_finite(self.lut_strength, 0.0, 1.0, 1.0),
        };
        let default_curve_x = [0.0, 0.25, 0.5, 0.75, 1.0];
        let monotonic = out.curve_x.windows(2).all(|pair| pair[1] > pair[0] + 1e-5)
            && (out.curve_x[0] - 0.0).abs() < 1e-5
            && (out.curve_x[4] - 1.0).abs() < 1e-5;
        if !monotonic {
            out.curve_x = default_curve_x;
            out.curve_y = default_curve_y();
        }
        if out.levels_in_black >= out.levels_in_white {
            out.levels_in_black = 0.0;
            out.levels_in_white = 1.0;
        }
        if out.levels_out_black >= out.levels_out_white {
            out.levels_out_black = 0.0;
            out.levels_out_white = 1.0;
        }
        out
    }

    pub fn is_neutral(&self) -> bool {
        self.sanitized() == Self::default()
    }

    pub fn uses_tone_curve(&self) -> bool {
        let p = self.sanitized();
        p.curve_y != default_curve_y()
            || p.levels_in_black != 0.0
            || p.levels_in_white != 1.0
            || p.levels_gamma != 1.0
            || p.levels_out_black != 0.0
            || p.levels_out_white != 1.0
    }

    pub fn uses_lut(&self) -> bool {
        let p = self.sanitized();
        p.lut_asset_id.is_some() && p.lut_strength > 0.0
    }

    /// Copy with the LUT reference cleared. `apply_color_adjustments` does not
    /// read LUT fields, so a neutral slider set must stay a bitwise no-op even
    /// when a look is attached at zero strength.
    pub fn without_lut(&self) -> Self {
        let mut out = *self;
        out.lut_asset_id = None;
        out.lut_strength = Self::default().lut_strength;
        out
    }
}

/// One facial feature ellipse in normalized source coordinates.
///
/// The fine-repair effects target features measured by the region analyser.
/// The geometry travels with the portrait layer so a document renders the same
/// way without re-running detection, and stays correct when the layer is
/// copied within a project.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PortraitEllipse {
    pub center_x: f32,
    pub center_y: f32,
    pub radius_x: f32,
    pub radius_y: f32,
    /// Rotation in radians.
    pub angle: f32,
    /// Edge falloff as a fraction of the radius, 0..1.
    pub feather: f32,
}

impl PortraitEllipse {
    pub fn sanitized(&self) -> Self {
        Self {
            center_x: clamp_finite(self.center_x, -0.5, 1.5, 0.5),
            center_y: clamp_finite(self.center_y, -0.5, 1.5, 0.5),
            radius_x: clamp_finite(self.radius_x, 0.0005, 2.0, 0.05),
            radius_y: clamp_finite(self.radius_y, 0.0005, 2.0, 0.05),
            angle: clamp_finite(self.angle, -std::f32::consts::PI, std::f32::consts::PI, 0.0),
            feather: clamp_finite(self.feather, 0.0, 1.0, 0.4),
        }
    }
}

/// Facial feature geometry used by the fine-repair effects. All values are
/// normalized to the uncropped source image.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PortraitGeometry {
    pub face: Option<PortraitEllipse>,
    pub left_eye: Option<PortraitEllipse>,
    pub right_eye: Option<PortraitEllipse>,
    pub left_brow: Option<PortraitEllipse>,
    pub right_brow: Option<PortraitEllipse>,
    pub mouth: Option<PortraitEllipse>,
    pub lips: Option<PortraitEllipse>,
    pub teeth: Option<PortraitEllipse>,
    /// Neck area below the face, used by neck wrinkle repair on body layers.
    pub neck: Option<PortraitEllipse>,
}

impl PortraitGeometry {
    pub fn sanitized(&self) -> Self {
        let clean = |value: Option<PortraitEllipse>| value.map(|shape| shape.sanitized());
        Self {
            face: clean(self.face),
            left_eye: clean(self.left_eye),
            right_eye: clean(self.right_eye),
            left_brow: clean(self.left_brow),
            right_brow: clean(self.right_brow),
            mouth: clean(self.mouth),
            lips: clean(self.lips),
            teeth: clean(self.teeth),
            neck: clean(self.neck),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Portrait parameters. The fine-repair fields (`eye_enhance` onward) use the
/// measured `geometry`; without geometry the pipeline keeps the conservative
/// skin-only behaviour rather than guessing where eyes or teeth are.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortraitParams {
    /// 0..100, overall strength, default 78 (matches the conservative UI).
    pub intensity: f32,
    pub smooth: f32,
    /// 0..100 detail retention, default 70.
    pub texture: f32,
    pub even_skin: f32,
    pub brighten: f32,
    pub body_smooth: f32,
    pub eye_enhance: f32,
    pub teeth_whiten: f32,
    pub blemish: f32,
    pub wrinkle: f32,
    pub dark_circle: f32,
    pub mole: f32,
    pub freckle: f32,
    pub birthmark: f32,
    pub tattoo: f32,
    /// Natural skin whitening: lifts luminance and neutralizes a yellow cast
    /// without promising to change the subject's actual skin tone.
    pub whiten: f32,
    /// -100 (cool) .. 100 (warm), applied by the whitening pass.
    pub whiten_warmth: f32,
    /// Subtle warm/rosy tint, strongest on the cheeks when geometry exists.
    pub rosy: f32,
    /// Under-eye bag shadow correction; structural reshaping stays manual.
    pub eye_bag: f32,
    /// Per-zone fine-line strengths. The legacy `wrinkle` master still applies
    /// to every zone unless a zone sets its own strength.
    pub wrinkle_forehead: f32,
    pub wrinkle_glabella: f32,
    pub wrinkle_crow_feet: f32,
    pub wrinkle_nasolabial: f32,
    pub wrinkle_mouth_corner: f32,
    pub wrinkle_under_eye: f32,
    pub wrinkle_neck: f32,
    /// A body-beauty layer. Its effects use the body skin mask instead of the
    /// face mask, and it never requires a face index.
    pub body_mode: bool,
    pub body_blemish: f32,
    pub body_even: f32,
    pub body_brighten: f32,
    pub body_texture: f32,
    pub body_whiten: f32,
    /// Horizontal narrowing inside the layer mask (0..100). The mask is the
    /// user's confirmation of the reshape area; the face ellipse is protected.
    pub body_slim: f32,
    /// Vertical stretch of the lower half of the mask (0..100). Used for legs;
    /// the mask is the user's confirmation of what may become taller.
    pub body_lengthen: f32,
    /// Local geometric shaping. Applied by the renderer before the colour
    /// pass; all three need face geometry.
    pub face_slim: f32,
    pub eye_size: f32,
    pub nose_slim: f32,
    /// 0..100; 0 keeps the automatic radius.
    pub smooth_radius: f32,
    /// 0..100 extra desaturation for a translucent whitening look.
    pub whiten_saturation: f32,
    /// 0..100, compresses specular highlights on skin.
    pub oil_control: f32,
    pub eye_white: f32,
    pub iris_detail: f32,
    /// Extra luminance inside the teeth gate, separate from yellow removal.
    pub teeth_brighten: f32,
    pub lip_tint: f32,
    /// -100 cool/berry .. 100 warm/coral.
    pub lip_warmth: f32,
    pub nose_contour: f32,
    /// Warm eyeshadow on the upper lid.
    pub eyeshadow: f32,
    /// Brows are darkened and warmed so they frame the eyes.
    pub brow_tint: f32,
    pub jaw_width: f32,
    /// -100 shorter .. 100 longer.
    pub chin_length: f32,
    /// -100 closer .. 100 wider.
    pub eye_distance: f32,
    /// Index of the detected face this layer was created for. `None` keeps the
    /// legacy global portrait layer behavior.
    #[serde(default)]
    pub face_index: Option<u32>,
    /// Measured feature geometry for the fine-repair effects.
    #[serde(default)]
    pub geometry: Option<PortraitGeometry>,
}

impl Default for PortraitParams {
    fn default() -> Self {
        Self {
            intensity: 78.0,
            smooth: 0.0,
            texture: 70.0,
            even_skin: 0.0,
            brighten: 0.0,
            body_smooth: 0.0,
            eye_enhance: 0.0,
            teeth_whiten: 0.0,
            blemish: 0.0,
            wrinkle: 0.0,
            dark_circle: 0.0,
            mole: 0.0,
            freckle: 0.0,
            birthmark: 0.0,
            tattoo: 0.0,
            whiten: 0.0,
            whiten_warmth: 0.0,
            rosy: 0.0,
            eye_bag: 0.0,
            wrinkle_forehead: 0.0,
            wrinkle_glabella: 0.0,
            wrinkle_crow_feet: 0.0,
            wrinkle_nasolabial: 0.0,
            wrinkle_mouth_corner: 0.0,
            wrinkle_under_eye: 0.0,
            wrinkle_neck: 0.0,
            body_mode: false,
            body_blemish: 0.0,
            body_even: 0.0,
            body_brighten: 0.0,
            body_texture: 0.0,
            body_whiten: 0.0,
            body_slim: 0.0,
            body_lengthen: 0.0,
            face_slim: 0.0,
            eye_size: 0.0,
            nose_slim: 0.0,
            smooth_radius: 0.0,
            whiten_saturation: 0.0,
            oil_control: 0.0,
            eye_white: 0.0,
            iris_detail: 0.0,
            teeth_brighten: 0.0,
            lip_tint: 0.0,
            lip_warmth: 0.0,
            nose_contour: 0.0,
            eyeshadow: 0.0,
            brow_tint: 0.0,
            jaw_width: 0.0,
            chin_length: 0.0,
            eye_distance: 0.0,
            face_index: None,
            geometry: None,
        }
    }
}

impl PortraitParams {
    pub fn sanitized(&self) -> Self {
        let c = |v: f32, fallback: f32| clamp_finite(v, 0.0, 100.0, fallback);
        Self {
            intensity: c(self.intensity, 78.0),
            smooth: c(self.smooth, 0.0),
            texture: c(self.texture, 70.0),
            even_skin: c(self.even_skin, 0.0),
            brighten: c(self.brighten, 0.0),
            body_smooth: c(self.body_smooth, 0.0),
            eye_enhance: c(self.eye_enhance, 0.0),
            teeth_whiten: c(self.teeth_whiten, 0.0),
            blemish: c(self.blemish, 0.0),
            wrinkle: c(self.wrinkle, 0.0),
            dark_circle: c(self.dark_circle, 0.0),
            mole: c(self.mole, 0.0),
            freckle: c(self.freckle, 0.0),
            birthmark: c(self.birthmark, 0.0),
            tattoo: c(self.tattoo, 0.0),
            whiten: c(self.whiten, 0.0),
            whiten_warmth: clamp_finite(self.whiten_warmth, -100.0, 100.0, 0.0),
            rosy: c(self.rosy, 0.0),
            eye_bag: c(self.eye_bag, 0.0),
            wrinkle_forehead: c(self.wrinkle_forehead, 0.0),
            wrinkle_glabella: c(self.wrinkle_glabella, 0.0),
            wrinkle_crow_feet: c(self.wrinkle_crow_feet, 0.0),
            wrinkle_nasolabial: c(self.wrinkle_nasolabial, 0.0),
            wrinkle_mouth_corner: c(self.wrinkle_mouth_corner, 0.0),
            wrinkle_under_eye: c(self.wrinkle_under_eye, 0.0),
            wrinkle_neck: c(self.wrinkle_neck, 0.0),
            body_mode: self.body_mode,
            body_blemish: c(self.body_blemish, 0.0),
            body_even: c(self.body_even, 0.0),
            body_brighten: c(self.body_brighten, 0.0),
            body_texture: c(self.body_texture, 0.0),
            body_whiten: c(self.body_whiten, 0.0),
            body_slim: c(self.body_slim, 0.0),
            body_lengthen: c(self.body_lengthen, 0.0),
            face_slim: c(self.face_slim, 0.0),
            eye_size: c(self.eye_size, 0.0),
            nose_slim: c(self.nose_slim, 0.0),
            smooth_radius: c(self.smooth_radius, 0.0),
            whiten_saturation: c(self.whiten_saturation, 0.0),
            oil_control: c(self.oil_control, 0.0),
            eye_white: c(self.eye_white, 0.0),
            iris_detail: c(self.iris_detail, 0.0),
            teeth_brighten: c(self.teeth_brighten, 0.0),
            lip_tint: c(self.lip_tint, 0.0),
            lip_warmth: clamp_finite(self.lip_warmth, -100.0, 100.0, 0.0),
            nose_contour: c(self.nose_contour, 0.0),
            eyeshadow: c(self.eyeshadow, 0.0),
            brow_tint: c(self.brow_tint, 0.0),
            jaw_width: c(self.jaw_width, 0.0),
            chin_length: clamp_finite(self.chin_length, -100.0, 100.0, 0.0),
            eye_distance: clamp_finite(self.eye_distance, -100.0, 100.0, 0.0),
            face_index: self.face_index.map(|index| index.min(31)),
            geometry: self.geometry.map(|geometry| geometry.sanitized()),
        }
    }

    /// Whether the pipeline has anything to do.
    pub fn has_active_effect(&self) -> bool {
        let p = self.sanitized();
        p.intensity > 0.0 && (p.has_face_effects() || p.has_body_effects())
    }

    /// Face-scoped beauty and fine-repair parameters.
    pub fn has_face_effects(&self) -> bool {
        let p = self.sanitized();
        p.smooth > 0.0
            || p.even_skin > 0.0
            || p.brighten > 0.0
            || p.whiten > 0.0
            || p.rosy > 0.0
            || p.eye_enhance > 0.0
            || p.eye_white > 0.0
            || p.iris_detail > 0.0
            || p.teeth_whiten > 0.0
            || p.teeth_brighten > 0.0
            || p.lip_tint > 0.0
            || p.nose_contour > 0.0
            || p.eyeshadow > 0.0
            || p.brow_tint > 0.0
            || p.oil_control > 0.0
            || p.blemish > 0.0
            || p.wrinkle > 0.0
            || p.dark_circle > 0.0
            || p.eye_bag > 0.0
            || p.mole > 0.0
            || p.freckle > 0.0
            || p.birthmark > 0.0
            || p.tattoo > 0.0
            || has_wrinkle_zones(&p)
    }

    /// Body-scoped beauty parameters.
    pub fn has_body_effects(&self) -> bool {
        let p = self.sanitized();
        p.body_smooth > 0.0
            || p.body_blemish > 0.0
            || p.body_even > 0.0
            || p.body_brighten > 0.0
            || p.body_texture > 0.0
            || p.body_whiten > 0.0
            || p.body_slim > 0.0
            || p.body_lengthen > 0.0
    }

    /// Whether the renderer must warp the frame before colour processing.
    pub fn has_shape_effect(&self) -> bool {
        let p = self.sanitized();
        p.face_slim > 0.0
            || p.eye_size > 0.0
            || p.nose_slim > 0.0
            || p.jaw_width > 0.0
            || p.chin_length.abs() > 0.0
            || p.eye_distance.abs() > 0.0
    }

    /// Effects handled by the fine-repair pass. `even_skin` and `brighten` run
    /// in the base pass; everything else is region- or mask-targeted there.
    pub fn has_fine_repair(&self) -> bool {
        let p = self.sanitized();
        p.eye_enhance > 0.0
            || p.eye_white > 0.0
            || p.iris_detail > 0.0
            || p.teeth_whiten > 0.0
            || p.teeth_brighten > 0.0
            || p.lip_tint > 0.0
            || p.nose_contour > 0.0
            || p.eyeshadow > 0.0
            || p.brow_tint > 0.0
            || p.oil_control > 0.0
            || p.blemish > 0.0
            || p.wrinkle > 0.0
            || p.dark_circle > 0.0
            || p.eye_bag > 0.0
            || p.rosy > 0.0
            || p.whiten > 0.0
            || p.mole > 0.0
            || p.freckle > 0.0
            || p.birthmark > 0.0
            || p.tattoo > 0.0
            || has_wrinkle_zones(&p)
            || p.body_smooth > 0.0
            || p.body_blemish > 0.0
            || p.body_even > 0.0
            || p.body_brighten > 0.0
            || p.body_texture > 0.0
            || p.body_whiten > 0.0
    }
}

fn has_wrinkle_zones(p: &PortraitParams) -> bool {
    p.wrinkle_forehead > 0.0
        || p.wrinkle_glabella > 0.0
        || p.wrinkle_crow_feet > 0.0
        || p.wrinkle_nasolabial > 0.0
        || p.wrinkle_mouth_corner > 0.0
        || p.wrinkle_under_eye > 0.0
        || p.wrinkle_neck > 0.0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskPoint {
    /// Normalized to the original source image (0..1).
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskStroke {
    pub points: Vec<MaskPoint>,
    /// Radius as a fraction of the source image's shortest side.
    pub radius: f32,
    pub feather: f32,
    pub erase: bool,
}

impl MaskStroke {
    pub fn sanitized(&self) -> Self {
        let mut stroke = self.clone();
        stroke.points.truncate(4096);
        for point in &mut stroke.points {
            point.x = clamp_finite(point.x, 0.0, 1.0, 0.5);
            point.y = clamp_finite(point.y, 0.0, 1.0, 0.5);
        }
        stroke.radius = clamp_finite(stroke.radius, 0.002, 0.25, 0.05);
        stroke.feather = clamp_finite(stroke.feather, 0.0, 1.0, 0.35);
        stroke
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskGradientKind {
    Linear,
    Radial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskGradient {
    pub kind: MaskGradientKind,
    /// Start point in normalized source coordinates; opaque end of a linear
    /// gradient or the center of a radial gradient.
    pub start: MaskPoint,
    /// End point in normalized source coordinates; transparent end or radius.
    pub end: MaskPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayerMask {
    pub strokes: Vec<MaskStroke>,
    /// Heuristic skin-tone selection. This is not semantic face segmentation.
    pub auto_skin: bool,
    /// Facial regions produced by the region analyser, in normalized source
    /// coordinates. They are plain ellipses here: the renderer never has to run
    /// detection, so a mask keeps working if no face is found later.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<MaskRegion>,
    /// Optional linear/radial gradient, combined with brush and skin coverage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient: Option<MaskGradient>,
    /// Invert the composed coverage.
    pub inverted: bool,
    /// Global edge softening, 0..1 of the shortest preview side.
    pub feather: f32,
    /// Overall mask strength, 0..1.
    pub density: f32,
}

impl Default for LayerMask {
    fn default() -> Self {
        Self {
            strokes: Vec::new(),
            auto_skin: false,
            regions: Vec::new(),
            gradient: None,
            inverted: false,
            feather: 0.0,
            density: 1.0,
        }
    }
}

impl LayerMask {
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
            && !self.auto_skin
            && self.gradient.is_none()
            && self.regions.is_empty()
    }
}

/// One soft elliptical facial region inside a layer mask.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MaskRegion {
    /// Analyser label, kept for display only ("leftEye", "lips", ...).
    pub kind: String,
    /// Normalized center in the uncropped source image.
    pub center_x: f32,
    pub center_y: f32,
    /// Normalized radii: x over width, y over height.
    pub radius_x: f32,
    pub radius_y: f32,
    /// Rotation in radians.
    pub angle: f32,
    /// Edge falloff as a fraction of the region, 0..1.
    pub feather: f32,
    /// Region density, 0..1.
    pub strength: f32,
}

impl Default for MaskRegion {
    fn default() -> Self {
        Self {
            kind: String::new(),
            center_x: 0.5,
            center_y: 0.5,
            radius_x: 0.05,
            radius_y: 0.05,
            angle: 0.0,
            feather: 0.4,
            strength: 1.0,
        }
    }
}

impl MaskRegion {
    pub fn sanitized(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            center_x: clamp_finite(self.center_x, -1.0, 2.0, 0.5),
            center_y: clamp_finite(self.center_y, -1.0, 2.0, 0.5),
            radius_x: clamp_finite(self.radius_x, 0.001, 2.0, 0.05),
            radius_y: clamp_finite(self.radius_y, 0.001, 2.0, 0.05),
            angle: clamp_finite(self.angle, -std::f32::consts::PI, std::f32::consts::PI, 0.0),
            feather: clamp_finite(self.feather, 0.0, 1.0, 0.4),
            strength: clamp_finite(self.strength, 0.0, 1.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub color: Option<ColorParams>,
    pub portrait: Option<PortraitParams>,
    #[serde(default)]
    pub retouch: Option<RetouchParams>,
    /// Optional manual brush mask. Applies to color/portrait layers.
    #[serde(default)]
    pub mask: Option<LayerMask>,
    /// Populated for `AiResult` layers.
    pub asset_id: Option<Uuid>,
    pub created_at: String,
}

impl Layer {
    fn with_kind(name: impl Into<String>, kind: LayerKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            kind,
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            color: None,
            portrait: None,
            retouch: None,
            mask: None,
            asset_id: None,
            created_at: now_rfc3339(),
        }
    }

    /// Base/source layer. It is always index 0 and cannot be deleted/reordered.
    pub fn base(name: impl Into<String>, asset_id: Option<Uuid>) -> Self {
        let mut layer = Self::with_kind(name, LayerKind::Base);
        layer.asset_id = asset_id;
        layer
    }

    pub fn portrait() -> Self {
        let mut layer = Self::with_kind("人像精修", LayerKind::Portrait);
        layer.portrait = Some(PortraitParams::default());
        layer
    }

    pub fn retouch() -> Self {
        let mut layer = Self::with_kind("局部修复", LayerKind::Retouch);
        layer.retouch = Some(RetouchParams::default());
        layer
    }

    pub fn color() -> Self {
        let mut layer = Self::with_kind("调色调整", LayerKind::Color);
        layer.color = Some(ColorParams::default());
        layer
    }

    pub fn ai_result(asset_id: Uuid) -> Self {
        let mut layer = Self::with_kind("AI 效果", LayerKind::AiResult);
        layer.asset_id = Some(asset_id);
        layer
    }

    pub fn sanitize(&mut self) {
        self.opacity = clamp_finite(self.opacity, 0.0, 1.0, 1.0);
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            self.name = "图层".to_string();
        }
        if let Some(color) = self.color {
            self.color = Some(color.sanitized());
        }
        if let Some(portrait) = self.portrait {
            self.portrait = Some(portrait.sanitized());
        }
        if let Some(retouch) = &mut self.retouch {
            retouch.spots.truncate(400);
            for spot in &mut retouch.spots {
                *spot = spot.sanitized();
            }
            retouch.warp.truncate(200);
            for stroke in &mut retouch.warp {
                *stroke = stroke.sanitized();
            }
            retouch.warp.retain(|stroke| !stroke.points.is_empty());
            retouch.freeze.truncate(200);
            for stroke in &mut retouch.freeze {
                *stroke = stroke.sanitized();
            }
            retouch.freeze.retain(|stroke| !stroke.points.is_empty());
            retouch
                .freeze_grid
                .truncate(FREEZE_GRID_COLS * FREEZE_GRID_ROWS);
        }
        if let Some(mask) = &mut self.mask {
            mask.regions.truncate(32);
            for region in &mut mask.regions {
                *region = region.sanitized();
            }
            mask.strokes.truncate(500);
            for stroke in &mut mask.strokes {
                *stroke = stroke.sanitized();
            }
            mask.strokes.retain(|stroke| !stroke.points.is_empty());
            if let Some(gradient) = &mut mask.gradient {
                for point in [&mut gradient.start, &mut gradient.end] {
                    point.x = clamp_finite(point.x, 0.0, 1.0, 0.5);
                    point.y = clamp_finite(point.y, 0.0, 1.0, 0.5);
                }
                let dx = gradient.end.x - gradient.start.x;
                let dy = gradient.end.y - gradient.start.y;
                if (dx * dx + dy * dy).sqrt() < 0.01 {
                    mask.gradient = None;
                }
            }
            mask.feather = clamp_finite(mask.feather, 0.0, 1.0, 0.0);
            mask.density = clamp_finite(mask.density, 0.0, 1.0, 1.0);
            if mask.is_empty() {
                self.mask = None;
            }
        }
    }

    pub fn apply_patch(&mut self, patch: &LayerPatch) {
        if let Some(name) = patch.name.as_ref() {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                self.name = trimmed.to_string();
            }
        }
        if let Some(visible) = patch.visible {
            self.visible = visible;
        }
        if let Some(locked) = patch.locked {
            self.locked = locked;
        }
        if let Some(opacity) = patch.opacity {
            self.opacity = clamp_finite(opacity, 0.0, 1.0, self.opacity);
        }
        if let Some(blend_mode) = patch.blend_mode {
            self.blend_mode = blend_mode;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayerPatch {
    pub name: Option<String>,
    pub visible: Option<bool>,
    pub locked: Option<bool>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<BlendMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MoveDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocument {
    pub id: Uuid,
    pub name: String,
    /// Photos sharing this id form one project/photo set. Old documents that
    /// predate collections normalize to a single-photo collection of their own.
    #[serde(default)]
    pub collection_id: Uuid,
    #[serde(default)]
    pub collection_name: String,
    pub schema_version: u32,
    /// Monotonic revision used to discard stale previews.
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub canvas: Canvas,
    /// Non-destructive crop applied after layer compositing.
    #[serde(default)]
    pub crop: CropRect,
    pub source: Option<SourceAsset>,
    /// Render order, bottom -> top. Base layer is index 0.
    pub layers: Vec<Layer>,
}

impl ProjectDocument {
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        let now = now_rfc3339();
        let id = Uuid::new_v4();
        let name = name.into();
        Self {
            id,
            collection_id: id,
            collection_name: name.clone(),
            name,
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 0,
            created_at: now.clone(),
            updated_at: now,
            canvas: Canvas::new(width, height),
            crop: CropRect::full(width, height),
            source: None,
            layers: Vec::new(),
        }
    }

    pub fn empty_project(name: impl Into<String>) -> Self {
        Self::new(name, 0, 0)
    }

    /// Clamp parameters / opacity and pin the base layer at index 0.
    pub fn normalized(mut self) -> Self {
        if self.schema_version < CURRENT_SCHEMA_VERSION {
            self.schema_version = CURRENT_SCHEMA_VERSION;
        }
        if self.collection_id.is_nil() {
            self.collection_id = self.id;
        }
        if self.collection_name.trim().is_empty() {
            self.collection_name = self.name.clone();
        }
        if self.canvas.working_color_space != WORKING_COLOR_SPACE {
            self.canvas.working_color_space = WORKING_COLOR_SPACE.to_string();
        }
        self.crop = if self.canvas.width == 0 || self.canvas.height == 0 {
            CropRect::default()
        } else if self.crop.is_empty() {
            CropRect::full(self.canvas.width, self.canvas.height)
        } else {
            self.crop.clamped(self.canvas.width, self.canvas.height)
        };
        for layer in &mut self.layers {
            layer.sanitize();
        }
        if let Some(pos) = self.layers.iter().position(|l| l.kind == LayerKind::Base) {
            if pos != 0 {
                let base = self.layers.remove(pos);
                self.layers.insert(0, base);
            }
        }
        self
    }

    pub fn base_layer(&self) -> Option<&Layer> {
        self.layers.first().filter(|l| l.kind == LayerKind::Base)
    }

    pub fn layer(&self, id: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: Uuid) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Compare document content while ignoring volatile revision/timestamps.
    pub fn content_eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.schema_version == other.schema_version
            && self.canvas == other.canvas
            && self.crop == other.crop
            && self.source == other.source
            && self.layers == other.layers
    }

    pub fn set_crop(&mut self, crop: CropRect) {
        self.crop = if self.canvas.width == 0 || self.canvas.height == 0 {
            CropRect::default()
        } else {
            crop.clamped(self.canvas.width, self.canvas.height)
        };
    }

    pub fn add_layer(&mut self, layer: Layer) -> Result<(), DocumentError> {
        if layer.kind == LayerKind::Base {
            return Err(DocumentError::BaseLayerProtected);
        }
        self.layers.push(layer);
        Ok(())
    }

    /// Duplicate a non-base layer directly above the original. The duplicate
    /// starts unlocked so it can be edited even when the source was locked.
    pub fn duplicate_layer(&mut self, layer_id: Uuid) -> Result<Layer, DocumentError> {
        let pos = self
            .layers
            .iter()
            .position(|layer| layer.id == layer_id)
            .ok_or(DocumentError::LayerNotFound)?;
        if self.layers[pos].kind == LayerKind::Base || pos == 0 {
            return Err(DocumentError::BaseLayerProtected);
        }
        let mut copy = self.layers[pos].clone();
        copy.id = Uuid::new_v4();
        copy.name = format!("{} 副本", self.layers[pos].name);
        copy.locked = false;
        copy.created_at = now_rfc3339();
        self.layers.insert(pos + 1, copy.clone());
        Ok(copy)
    }

    pub fn delete_layer(&mut self, layer_id: Uuid) -> Result<(), DocumentError> {
        let pos = self
            .layers
            .iter()
            .position(|l| l.id == layer_id)
            .ok_or(DocumentError::LayerNotFound)?;
        if self.layers[pos].kind == LayerKind::Base || pos == 0 {
            return Err(DocumentError::BaseLayerProtected);
        }
        self.layers.remove(pos);
        Ok(())
    }

    /// Swaps with the adjacent layer. The base layer remains pinned at index 0;
    /// moving the lowest adjustable layer down is a no-op.
    pub fn move_layer(
        &mut self,
        layer_id: Uuid,
        direction: MoveDirection,
    ) -> Result<bool, DocumentError> {
        let pos = self
            .layers
            .iter()
            .position(|l| l.id == layer_id)
            .ok_or(DocumentError::LayerNotFound)?;
        if self.layers[pos].kind == LayerKind::Base || pos == 0 {
            return Err(DocumentError::BaseLayerProtected);
        }
        match direction {
            MoveDirection::Up => {
                if pos + 1 < self.layers.len() {
                    self.layers.swap(pos, pos + 1);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            MoveDirection::Down => {
                // Never swap with (or move onto) the base layer.
                if pos > 1 {
                    self.layers.swap(pos, pos - 1);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DocumentError {
    #[error("background layer is protected")]
    BaseLayerProtected,
    #[error("layer not found")]
    LayerNotFound,
    #[error("invalid layer operation: {0}")]
    InvalidLayer(String),
}

impl DocumentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BaseLayerProtected => "BASE_LAYER_PROTECTED",
            Self::LayerNotFound => "LAYER_NOT_FOUND",
            Self::InvalidLayer(_) => "INVALID_ARGUMENT",
        }
    }

    pub fn user_message(&self) -> &'static str {
        match self {
            Self::BaseLayerProtected => "背景图层不能删除，也不能调整顺序。",
            Self::LayerNotFound => "图层不存在，可能已被删除。",
            Self::InvalidLayer(_) => "图层操作无效。",
        }
    }
}

/// Undo steps kept per open project. History is session-only, but every entry
/// holds a whole `ProjectDocument`, and a document with hundreds of mask strokes
/// is worth megabytes — without a cap a long editing session grows unbounded.
const MAX_HISTORY: usize = 60;
/// Shared by undo + redo per open project. Sixty mask-heavy snapshots can still
/// exceed this, so the count cap alone is not a memory bound.
const MAX_HISTORY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
struct HistoryEntry {
    label: String,
    snapshot: ProjectDocument,
    /// Approximate heap + inline size, computed once when the snapshot is made.
    bytes: usize,
}

impl HistoryEntry {
    fn new(label: String, snapshot: ProjectDocument) -> Self {
        let bytes =
            std::mem::size_of::<Self>() + label.capacity() + estimated_document_bytes(&snapshot);
        Self {
            label,
            snapshot,
            bytes,
        }
    }
}

/// Cheap structural estimate, not a serialization: count fixed struct sizes and
/// the heap capacities that dominate masks/retouch history snapshots.
fn estimated_document_bytes(document: &ProjectDocument) -> usize {
    let mut bytes = std::mem::size_of::<ProjectDocument>()
        + document.name.capacity()
        + document.created_at.capacity()
        + document.updated_at.capacity()
        + document.canvas.working_color_space.capacity();
    if let Some(source) = &document.source {
        bytes += std::mem::size_of::<SourceAsset>()
            + source.display_name.capacity()
            + source.format.capacity()
            + source.original_path.capacity()
            + source.color_space.capacity();
    }
    bytes += document.layers.capacity() * std::mem::size_of::<Layer>();
    for layer in &document.layers {
        bytes += layer.name.capacity() + layer.created_at.capacity();
        if let Some(retouch) = &layer.retouch {
            bytes += retouch.spots.capacity() * std::mem::size_of::<RetouchSpot>();
        }
        if let Some(mask) = &layer.mask {
            bytes += mask.strokes.capacity() * std::mem::size_of::<MaskStroke>()
                + mask.regions.capacity() * std::mem::size_of::<MaskRegion>();
            for stroke in &mask.strokes {
                bytes += stroke.points.capacity() * std::mem::size_of::<MaskPoint>();
            }
            for region in &mask.regions {
                bytes += region.kind.capacity();
            }
        }
    }
    bytes
}

fn push_capped(stack: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    stack.push(entry);
    if stack.len() > MAX_HISTORY {
        stack.drain(0..stack.len() - MAX_HISTORY);
    }
}

/// In-memory editing session.
///
/// `mutate` bumps the revision on every change. `commit` is the only operation
/// that creates an undo entry, which is how a continuous slider drag collapses
/// into a single user-visible history step. History itself is session-only and
/// is intentionally not persisted to SQLite; document parameters are.
#[derive(Debug, Clone)]
pub struct DocumentSession {
    doc: ProjectDocument,
    baseline: ProjectDocument,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl DocumentSession {
    pub fn new(doc: ProjectDocument) -> Self {
        let doc = doc.normalized();
        Self {
            baseline: doc.clone(),
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn document(&self) -> &ProjectDocument {
        &self.doc
    }

    pub fn document_mut(&mut self) -> &mut ProjectDocument {
        &mut self.doc
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Rename the project without creating an undo entry or invalidating the
    /// pixel cache. The name is metadata, not an image edit.
    pub fn rename(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.doc.name = name.clone();
        self.baseline.name = name;
    }

    /// Rename the photo set. Metadata only: no undo entry, no revision bump,
    /// so existing preview caches stay valid.
    pub fn rename_collection(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.doc.collection_name = name.clone();
        self.baseline.collection_name = name;
    }

    /// Labels of undoable actions, oldest first.
    pub fn history(&self) -> Vec<String> {
        self.undo.iter().map(|e| e.label.clone()).collect()
    }

    fn bump(&mut self) {
        self.doc.revision = self.doc.revision.saturating_add(1);
        self.doc.updated_at = now_rfc3339();
    }

    /// Apply a mutation without creating history. The revision always advances,
    /// even if the closure ended up writing an identical value, so preview
    /// caches derived from the revision can never go stale.
    pub fn mutate<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce(&mut ProjectDocument) -> T,
    {
        let out = f(&mut self.doc);
        self.bump();
        out
    }

    /// Fallible variant that leaves revision untouched on error.
    pub fn try_mutate<F, T, E>(&mut self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut ProjectDocument) -> Result<T, E>,
    {
        let out = f(&mut self.doc)?;
        self.bump();
        Ok(out)
    }

    /// Collapse all changes since the last commit/baseline into one undo entry.
    /// Returns true when an entry was actually pushed.
    pub fn commit(&mut self, label: &str) -> bool {
        if self.doc.content_eq(&self.baseline) {
            return false;
        }
        let label = {
            let trimmed = label.trim();
            if trimmed.is_empty() {
                "调整".to_string()
            } else {
                trimmed.to_string()
            }
        };
        // Move the old baseline into history instead of cloning it; the new
        // baseline must be a copy of the current document.
        let previous = std::mem::replace(&mut self.baseline, self.doc.clone());
        push_capped(&mut self.undo, HistoryEntry::new(label, previous));
        self.redo.clear();
        self.trim_history();
        true
    }

    fn history_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .map(|entry| entry.bytes)
            .sum()
    }

    /// Drop the oldest entries (undo first, then redo) until the shared budget
    /// fits. Recent undo steps are more valuable than forward history.
    fn trim_history_to(&mut self, max_bytes: usize) {
        let mut total = self.history_bytes();
        while total > max_bytes {
            let removed = if !self.undo.is_empty() {
                Some(self.undo.remove(0))
            } else if !self.redo.is_empty() {
                Some(self.redo.remove(0))
            } else {
                None
            };
            let Some(entry) = removed else {
                break;
            };
            total = total.saturating_sub(entry.bytes);
        }
    }

    fn trim_history(&mut self) {
        self.trim_history_to(MAX_HISTORY_BYTES);
    }

    /// Replace the current document with `snapshot` and return the previous
    /// document so the caller can move it onto the opposite stack. Moving avoids
    /// cloning a full document on every undo/redo.
    fn restore(&mut self, mut snapshot: ProjectDocument) -> ProjectDocument {
        let next = self.doc.revision.max(snapshot.revision).saturating_add(1);
        snapshot.revision = next;
        snapshot.updated_at = now_rfc3339();
        let next = snapshot.normalized();
        let previous = std::mem::replace(&mut self.doc, next);
        self.baseline = self.doc.clone();
        previous
    }

    /// Replace the document with a stored named snapshot.
    ///
    /// Identity stays owned by the live project (`id`, collection, photo name,
    /// `created_at`), so restoring an old version can never move a photo into a
    /// different set or resurrect a deleted one. The previous document goes onto
    /// the undo stack, which makes the restore itself undoable, and the revision
    /// advances past both documents so preview caches cannot collide.
    pub fn apply_snapshot(&mut self, mut snapshot: ProjectDocument, label: &str) {
        let live = self.doc.clone();
        snapshot.id = live.id;
        snapshot.collection_id = live.collection_id;
        snapshot.collection_name = live.collection_name.clone();
        snapshot.name = live.name.clone();
        snapshot.created_at = live.created_at.clone();
        let previous = self.restore(snapshot);
        push_capped(
            &mut self.undo,
            HistoryEntry::new(label.to_string(), previous),
        );
        self.redo.clear();
        self.trim_history();
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        let previous = self.restore(entry.snapshot);
        push_capped(&mut self.redo, HistoryEntry::new(entry.label, previous));
        self.trim_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        let previous = self.restore(entry.snapshot);
        push_capped(&mut self.undo, HistoryEntry::new(entry.label, previous));
        self.trim_history();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_base() -> DocumentSession {
        let mut doc = ProjectDocument::new("测试项目", 1200, 800);
        doc.source = Some(SourceAsset {
            asset_id: Uuid::new_v4(),
            display_name: "portrait.png".into(),
            width: 1200,
            height: 800,
            format: "png".into(),
            original_path: "C:/photos/portrait.png".into(),
            file_bytes: 1234,
            color_space: "sRGB".into(),
            icc_asset_id: None,
        });
        doc.layers.push(Layer::base("背景", None));
        DocumentSession::new(doc)
    }

    #[test]
    fn noop_commit_creates_no_history() {
        let mut session = session_with_base();
        let revision = session.document().revision;
        assert!(!session.commit("无变化"));
        assert!(!session.can_undo());
        assert!(session.history().is_empty());
        assert_eq!(session.document().revision, revision);
    }

    #[test]
    fn one_entry_per_drag() {
        let mut session = session_with_base();
        let color_layer = Layer::color();
        let color_id = color_layer.id;
        session.mutate(|doc| doc.add_layer(color_layer).unwrap());
        for i in 0..25 {
            session.mutate(|doc| {
                let layer = doc.layer_mut(color_id).unwrap();
                layer.color = Some(ColorParams {
                    exposure: i as f32 / 10.0,
                    ..Default::default()
                });
            });
        }
        assert!(session.commit("曝光"));
        assert_eq!(session.history(), vec!["曝光".to_string()]);
        assert!(!session.commit("重复提交"));
        assert_eq!(session.history().len(), 1);
    }

    #[test]
    fn history_is_capped_and_keeps_the_newest_steps() {
        let mut session = session_with_base();
        let color_layer = Layer::color();
        let color_id = color_layer.id;
        session.mutate(|doc| doc.add_layer(color_layer).unwrap());
        for i in 0..(MAX_HISTORY + 20) {
            session.mutate(|doc| {
                doc.layer_mut(color_id).unwrap().opacity = (i % 10) as f32 / 10.0;
            });
            assert!(session.commit(&format!("步骤 {i}")));
        }
        let history = session.history();
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history.first().unwrap(), "步骤 20");
        assert_eq!(
            history.last().unwrap(),
            &format!("步骤 {}", MAX_HISTORY + 19)
        );

        // Undo still walks back through the retained steps, and the steps that
        // were dropped are unreachable rather than half-restored.
        let mut undone = 0;
        while session.undo() {
            undone += 1;
        }
        assert_eq!(undone, MAX_HISTORY);
        assert!(!session.can_undo());
        // Redo walks back up the stack, so the first step back is the oldest
        // retained one; the redo stack is capped the same way.
        assert!(session.redo());
        assert_eq!(session.history(), vec!["步骤 20".to_string()]);
        assert!(session.can_undo());
    }

    #[test]
    fn history_byte_budget_evicts_oldest_snapshots() {
        let mut session = session_with_base();
        let mut layer = Layer::color();
        let layer_id = layer.id;
        layer.mask = Some(LayerMask {
            strokes: vec![MaskStroke {
                points: vec![MaskPoint { x: 0.5, y: 0.5 }; 2_000],
                radius: 0.1,
                feather: 0.2,
                erase: false,
            }],
            ..Default::default()
        });
        session.mutate(|doc| doc.add_layer(layer).unwrap());
        assert!(estimated_document_bytes(session.document()) > 16_000);

        for i in 0..12 {
            session.mutate(|doc| {
                doc.layer_mut(layer_id).unwrap().opacity = (i % 10) as f32 / 10.0;
            });
            assert!(session.commit(&format!("步骤 {i}")));
        }
        assert!(session.history_bytes() > 80_000);
        session.trim_history_to(80_000);
        assert!(session.history_bytes() <= 80_000);
        let retained = session.history().len();
        assert!(retained > 0 && retained < 12);

        let mut undone = 0;
        while session.undo() {
            undone += 1;
        }
        assert_eq!(undone, retained);
        assert!(session.can_redo());
        session.trim_history_to(1);
        assert!(!session.can_undo());
        assert!(!session.can_redo());
    }

    #[test]
    fn undo_redo_revisions_are_monotonic() {
        let mut session = session_with_base();
        let mut last = session.document().revision;
        session.mutate(|doc| {
            doc.layers.push(Layer::portrait());
        });
        assert!(session.commit("添加人像"));
        session.mutate(|doc| {
            doc.layer_mut(doc.layers[1].id).unwrap().opacity = 0.5;
        });
        assert!(session.commit("透明度"));
        assert!(session.can_undo());

        assert!(session.undo());
        assert!(session.document().revision > last);
        last = session.document().revision;
        assert!(session.can_redo());
        assert_eq!(session.history().len(), 1);

        assert!(session.redo());
        assert!(session.document().revision > last);
        last = session.document().revision;

        assert!(session.undo());
        assert!(session.document().revision > last);
    }

    #[test]
    fn crop_is_clamped_and_undoable() {
        let mut session = session_with_base();
        assert!(session.document().crop.is_full(1200, 800));

        session.mutate(|doc| {
            doc.set_crop(CropRect {
                x: 1100,
                y: 700,
                width: 500,
                height: 500,
                angle: 0.0,
            });
        });
        assert!(session.commit("裁剪照片"));
        let crop = session.document().crop;
        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (700, 300, 500, 500)
        );

        assert!(session.undo());
        assert!(session.document().crop.is_full(1200, 800));
        assert!(session.redo());
        assert_eq!(session.document().crop.width, 500);
    }

    #[test]
    fn old_documents_without_crop_default_to_full_frame() {
        let json = r#"{
            "id":"00000000-0000-4000-8000-000000000001",
            "name":"old",
            "schemaVersion":1,
            "revision":3,
            "createdAt":"2026-01-01T00:00:00.000Z",
            "updatedAt":"2026-01-01T00:00:00.000Z",
            "canvas":{"width":640,"height":480,"workingColorSpace":"linear-srgb"},
            "source":null,
            "layers":[]
        }"#;
        let doc: ProjectDocument = serde_json::from_str(json).unwrap();
        let normalized = doc.normalized();
        assert!(normalized.crop.is_full(640, 480));
        assert_eq!(normalized.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn colour_params_default_to_no_lut() {
        let legacy: ColorParams = serde_json::from_str(
            r#"{"exposure":0.5,"curveX":[0,0.25,0.5,0.75,1],"curveY":[0,0.25,0.5,0.75,1]}"#,
        )
        .unwrap();
        assert_eq!(legacy.lut_asset_id, None);
        assert_eq!(legacy.lut_strength, 1.0);
        assert!(!legacy.uses_lut());

        let with_lut = ColorParams {
            lut_asset_id: Some(Uuid::new_v4()),
            lut_strength: f32::NAN,
            ..Default::default()
        };
        assert_eq!(with_lut.sanitized().lut_strength, 1.0);
        assert!(with_lut.uses_lut());
        // Strength 0 keeps the reference but must not load or apply the LUT.
        assert!(!ColorParams {
            lut_strength: 0.0,
            ..with_lut
        }
        .uses_lut());
    }

    #[test]
    fn brush_mask_is_clamped_and_empty_masks_are_removed() {
        let mut layer = Layer::color();
        layer.mask = Some(LayerMask {
            strokes: vec![MaskStroke {
                points: vec![
                    MaskPoint { x: -2.0, y: 3.0 },
                    MaskPoint {
                        x: f32::NAN,
                        y: 0.5,
                    },
                ],
                radius: 9.0,
                feather: -1.0,
                erase: false,
            }],
            auto_skin: false,
            ..LayerMask::default()
        });
        layer.sanitize();
        let mask = layer.mask.expect("mask");
        assert_eq!(mask.strokes.len(), 1);
        assert_eq!(mask.strokes[0].radius, 0.25);
        assert_eq!(mask.strokes[0].feather, 0.0);
        assert_eq!(mask.strokes[0].points.len(), 2);
        assert_eq!(mask.strokes[0].points[0].x, 0.0);
        assert_eq!(mask.strokes[0].points[0].y, 1.0);
        assert_eq!(mask.strokes[0].points[1].x, 0.5);
        assert_eq!(mask.strokes[0].points[1].y, 0.5);

        layer.mask = Some(LayerMask::default());
        layer.sanitize();
        assert!(layer.mask.is_none());

        layer.mask = Some(LayerMask {
            strokes: Vec::new(),
            auto_skin: true,
            ..LayerMask::default()
        });
        layer.sanitize();
        assert!(layer.mask.is_some_and(|mask| mask.auto_skin));
    }

    #[test]
    fn duplicate_layer_copies_params_masks_and_protects_base() {
        let mut session = session_with_base();
        let mut layer = Layer::color();
        layer.color = Some(ColorParams {
            exposure: 0.8,
            ..Default::default()
        });
        layer.mask = Some(LayerMask {
            auto_skin: true,
            ..LayerMask::default()
        });
        layer.locked = true;
        let layer_id = layer.id;
        session.try_mutate(|doc| doc.add_layer(layer)).unwrap();
        session.commit("添加调色图层");

        let copy = session
            .try_mutate(|doc| doc.duplicate_layer(layer_id))
            .unwrap();
        assert_ne!(copy.id, layer_id);
        assert_eq!(
            copy.color,
            Some(ColorParams {
                exposure: 0.8,
                ..Default::default()
            })
        );
        assert!(copy.mask.is_some_and(|mask| mask.auto_skin));
        assert!(!copy.locked);
        assert!(copy.name.ends_with("副本"));
        assert_eq!(session.document().layers.len(), 3);
        assert_eq!(session.document().layers[2].id, copy.id);

        let base_id = session.document().layers[0].id;
        assert_eq!(
            session.try_mutate(|doc| doc.duplicate_layer(base_id)),
            Err(DocumentError::BaseLayerProtected)
        );
    }

    #[test]
    fn gradient_mask_is_clamped_and_requires_a_span() {
        let mut layer = Layer::color();
        layer.mask = Some(LayerMask {
            gradient: Some(MaskGradient {
                kind: MaskGradientKind::Linear,
                start: MaskPoint { x: -1.0, y: 2.0 },
                end: MaskPoint { x: 2.0, y: 0.5 },
            }),
            inverted: true,
            feather: 9.0,
            density: -1.0,
            ..LayerMask::default()
        });
        layer.sanitize();
        let mask = layer.mask.expect("mask");
        let gradient = mask.gradient.expect("gradient");
        assert_eq!(gradient.start.x, 0.0);
        assert_eq!(gradient.start.y, 1.0);
        assert_eq!(gradient.end.x, 1.0);
        assert_eq!(gradient.end.y, 0.5);
        assert!(mask.inverted);
        assert_eq!(mask.feather, 1.0);
        assert_eq!(mask.density, 0.0);

        layer.mask = Some(LayerMask {
            gradient: Some(MaskGradient {
                kind: MaskGradientKind::Radial,
                start: MaskPoint { x: 0.5, y: 0.5 },
                end: MaskPoint { x: 0.5, y: 0.5 },
            }),
            ..LayerMask::default()
        });
        layer.sanitize();
        assert!(layer.mask.is_none());
    }

    #[test]
    fn base_layer_is_protected() {
        let mut session = session_with_base();
        let base_id = session.document().layers[0].id;
        let result = session.try_mutate(|doc| doc.delete_layer(base_id));
        assert_eq!(result, Err(DocumentError::BaseLayerProtected));
        assert_eq!(session.document().layers[0].id, base_id);

        let result = session.try_mutate(|doc| doc.move_layer(base_id, MoveDirection::Up));
        assert_eq!(result, Err(DocumentError::BaseLayerProtected));

        session.mutate(|doc| {
            doc.layers.push(Layer::portrait());
            doc.layers.push(Layer::color());
        });
        let portrait_id = session.document().layers[1].id;
        // Lowest adjustable layer cannot move below/onto the base layer.
        let moved = session
            .mutate(|doc| doc.move_layer(portrait_id, MoveDirection::Down))
            .unwrap();
        assert!(!moved);
        assert_eq!(session.document().layers[0].id, base_id);
        assert_eq!(session.document().layers[1].id, portrait_id);
    }

    #[test]
    fn params_and_opacity_are_clamped() {
        let c = ColorParams {
            exposure: 99.0,
            contrast: f32::NAN,
            saturation: -500.0,
            sharpness: 300.0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(c.exposure, 3.0);
        assert_eq!(c.contrast, 0.0);
        assert_eq!(c.saturation, -100.0);
        assert_eq!(c.sharpness, 100.0);

        let p = PortraitParams {
            intensity: 900.0,
            texture: f32::INFINITY,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(p.intensity, 100.0);
        assert_eq!(p.texture, 70.0);
        assert_eq!(PortraitParams::default().intensity, 78.0);
        assert_eq!(PortraitParams::default().texture, 70.0);
        assert!(!PortraitParams::default().has_active_effect());
        assert!(PortraitParams {
            eye_enhance: 40.0,
            ..Default::default()
        }
        .has_active_effect());
        assert!(PortraitParams {
            body_mode: true,
            body_blemish: 40.0,
            ..Default::default()
        }
        .has_active_effect());
        let warmth = PortraitParams {
            whiten_warmth: 250.0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(warmth.whiten_warmth, 100.0);
        let with_geometry = PortraitParams {
            blemish: 40.0,
            geometry: Some(PortraitGeometry {
                face: Some(PortraitEllipse {
                    center_x: 9.0,
                    radius_x: f32::NAN,
                    feather: 3.0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
        .sanitized();
        let face = with_geometry.geometry.unwrap().face.unwrap();
        assert_eq!(face.center_x, 1.5);
        assert_eq!(face.radius_x, 0.05);
        assert_eq!(face.feather, 1.0);

        let mut layer = Layer::color();
        layer.apply_patch(&LayerPatch {
            opacity: Some(7.0),
            ..Default::default()
        });
        layer.sanitize();
        assert_eq!(layer.opacity, 1.0);
    }
}
