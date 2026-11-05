import type {
  MaskRegion,
  PortraitEllipse,
  PortraitFace,
  PortraitFaceRegions,
  PortraitGeometry,
  PortraitRegion,
} from "../api/types";

/**
 * CenterFace/YuNet expose a 0..1 detector confidence. The backend merges the
 * two models (dual-model agreement uses their average), so the UI can show the
 * value directly as a percentage. It is still a detector score rather than a
 * calibrated probability.
 */
export function faceScorePercent(score: number): number {
  const safe = Number.isFinite(score) ? Math.min(1, Math.max(0, score)) : 0;
  return Math.round(safe * 100);
}

export type FaceQuality = "高" | "中" | "低";

export function faceQuality(score: number): FaceQuality {
  const percent = faceScorePercent(score);
  if (percent >= 75) return "高";
  if (percent >= 55) return "中";
  return "低";
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

/**
 * Mask for one detected face. Prefer the measured skin ellipse; otherwise use
 * the detector box as a soft face oval so per-face beauty still works before
 * the region analyser has run.
 */
export function faceMaskRegion(
  face: PortraitFace,
  sourceWidth: number,
  sourceHeight: number,
  measured?: PortraitFaceRegions | null,
): MaskRegion {
  const width = Math.max(1, sourceWidth);
  const height = Math.max(1, sourceHeight);
  const skin = measured?.regions.find((region) => region.kind === "skin");
  if (skin) {
    return {
      kind: "face",
      centerX: clamp01(skin.centerX),
      centerY: clamp01(skin.centerY),
      radiusX: Math.max(0.005, Math.min(1, skin.radiusX)),
      radiusY: Math.max(0.005, Math.min(1, skin.radiusY)),
      angle: skin.angle,
      feather: Math.max(0.3, skin.feather),
      strength: 1,
    };
  }
  return {
    kind: "face",
    centerX: clamp01((face.x + face.width / 2) / width),
    centerY: clamp01((face.y + face.height / 2) / height),
    radiusX: Math.max(0.005, Math.min(1, (face.width * 0.52) / width)),
    radiusY: Math.max(0.005, Math.min(1, (face.height * 0.62) / height)),
    angle: 0,
    feather: 0.55,
    strength: 1,
  };
}

function ellipseOf(region: PortraitRegion): PortraitEllipse {
  return {
    centerX: clamp01(region.centerX),
    centerY: clamp01(region.centerY),
    radiusX: Math.max(0.0005, Math.min(1, region.radiusX)),
    radiusY: Math.max(0.0005, Math.min(1, region.radiusY)),
    angle: region.angle,
    feather: Math.max(0.2, Math.min(1, region.feather)),
  };
}

/**
 * Geometry for the fine-repair effects. Measured eye/mouth/teeth ellipses are
 * used when the region analyser has run; otherwise the face oval lets the
 * backend place the features by proportion until a measurement arrives.
 */
export function portraitGeometryForFace(
  face: PortraitFace,
  sourceWidth: number,
  sourceHeight: number,
  measured?: PortraitFaceRegions | null,
): PortraitGeometry {
  const find = (kind: string): PortraitEllipse | null => {
    const region = measured?.regions.find((item) => item.kind === kind);
    return region ? ellipseOf(region) : null;
  };
  const skin = measured?.regions.find((region) => region.kind === "skin");
  const faceEllipse: PortraitEllipse = skin
    ? ellipseOf(skin)
    : {
        centerX: clamp01((face.x + face.width / 2) / Math.max(1, sourceWidth)),
        centerY: clamp01((face.y + face.height / 2) / Math.max(1, sourceHeight)),
        radiusX: Math.max(0.005, Math.min(1, (face.width * 0.52) / Math.max(1, sourceWidth))),
        radiusY: Math.max(0.005, Math.min(1, (face.height * 0.62) / Math.max(1, sourceHeight))),
        angle: 0,
        feather: 0.55,
      };
  return {
    face: faceEllipse,
    leftEye: find("leftEye"),
    rightEye: find("rightEye"),
    leftBrow: find("leftBrow"),
    rightBrow: find("rightBrow"),
    mouth: find("mouth"),
    lips: find("lips"),
    teeth: find("teeth"),
    neck: find("neck"),
  };
}

export function hasFineEffects(params: {
  eyeEnhance: number;
  teethWhiten: number;
  blemish: number;
  wrinkle: number;
  darkCircle: number;
  mole: number;
  freckle: number;
  birthmark: number;
  tattoo: number;
  whiten: number;
  rosy: number;
  eyeBag: number;
  wrinkleForehead: number;
  wrinkleGlabella: number;
  wrinkleCrowFeet: number;
  wrinkleNasolabial: number;
  wrinkleMouthCorner: number;
  wrinkleUnderEye: number;
  wrinkleNeck: number;
  faceSlim: number;
  eyeSize: number;
  noseSlim: number;
  oilControl: number;
  eyeWhite: number;
  irisDetail: number;
  teethBrighten: number;
  lipTint: number;
  noseContour: number;
  eyeshadow: number;
  browTint: number;
  jawWidth: number;
  chinLength: number;
  eyeDistance: number;
}): boolean {
  return (
    params.eyeEnhance > 0 ||
    params.teethWhiten > 0 ||
    params.blemish > 0 ||
    params.wrinkle > 0 ||
    params.darkCircle > 0 ||
    params.mole > 0 ||
    params.freckle > 0 ||
    params.birthmark > 0 ||
    params.tattoo > 0 ||
    params.whiten > 0 ||
    params.rosy > 0 ||
    params.eyeBag > 0 ||
    params.wrinkleForehead > 0 ||
    params.wrinkleGlabella > 0 ||
    params.wrinkleCrowFeet > 0 ||
    params.wrinkleNasolabial > 0 ||
    params.wrinkleMouthCorner > 0 ||
    params.wrinkleUnderEye > 0 ||
    params.wrinkleNeck > 0 ||
    params.faceSlim > 0 ||
    params.eyeSize > 0 ||
    params.noseSlim > 0 ||
    params.oilControl > 0 ||
    params.eyeWhite > 0 ||
    params.irisDetail > 0 ||
    params.teethBrighten > 0 ||
    params.lipTint > 0 ||
    params.noseContour > 0 ||
    params.eyeshadow > 0 ||
    params.browTint > 0 ||
    params.jawWidth > 0 ||
    params.chinLength !== 0 ||
    params.eyeDistance !== 0
  );
}

export function faceLayerName(index: number, total: number): string {
  return total > 1 ? `人脸 ${index + 1} 美颜` : "人像美颜";
}

/** Default selection: the strongest detection, while list order stays visual. */
export function bestFaceIndex(faces: Array<{ confidence: number }>): number {
  if (faces.length === 0) return 0;
  return faces.reduce(
    (best, face, index) => (face.confidence > faces[best].confidence ? index : best),
    0,
  );
}
