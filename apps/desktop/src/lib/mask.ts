import { rotatedFrameSize } from "./crop";
import type {
  CropState,
  LayerMask,
  MaskGradient,
  MaskGradientKind,
  MaskPoint,
} from "../api/types";

export const REGION_KIND_LABELS: Record<string, string> = {
  face: "人脸范围",
  skin: "脸部皮肤",
  leftEye: "左眼",
  rightEye: "右眼",
  leftBrow: "左眉",
  rightBrow: "右眉",
  mouth: "嘴部",
  lips: "嘴唇",
  teeth: "牙齿",
  nose: "鼻部",
};

/** Region kinds worth turning into a mask, in display order. */
export const MASKABLE_REGIONS = [
  "leftEye",
  "rightEye",
  "leftBrow",
  "rightBrow",
  "lips",
  "teeth",
  "skin",
] as const;

export function regionLabel(kind: string): string {
  return REGION_KIND_LABELS[kind] ?? kind;
}

export function viewPointToSource(
  point: MaskPoint,
  crop: CropState,
  sourceWidth: number,
  sourceHeight: number,
  angle = 0,
): MaskPoint {
  const frameX = crop.x + point.x * crop.width;
  const frameY = crop.y + point.y * crop.height;
  if (Math.abs(angle) < 1e-4) {
    return {
      x: frameX / Math.max(1, sourceWidth),
      y: frameY / Math.max(1, sourceHeight),
    };
  }
  // Undo the straighten rotation: the view is in the rotated bounding box.
  const frame = rotatedFrameSize(sourceWidth, sourceHeight, angle);
  const radians = (angle * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const dx = frameX - frame.width / 2;
  const dy = frameY - frame.height / 2;
  return {
    x: (sourceWidth / 2 + dx * cos + dy * sin) / Math.max(1, sourceWidth),
    y: (sourceHeight / 2 - dx * sin + dy * cos) / Math.max(1, sourceHeight),
  };
}

export function sourcePointToView(
  point: MaskPoint,
  crop: CropState,
  sourceWidth: number,
  sourceHeight: number,
  angle = 0,
): MaskPoint {
  if (Math.abs(angle) < 1e-4) {
    return {
      x: (point.x * sourceWidth - crop.x) / Math.max(1, crop.width),
      y: (point.y * sourceHeight - crop.y) / Math.max(1, crop.height),
    };
  }
  const frame = rotatedFrameSize(sourceWidth, sourceHeight, angle);
  const radians = (angle * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const dx = point.x * sourceWidth - sourceWidth / 2;
  const dy = point.y * sourceHeight - sourceHeight / 2;
  const frameX = frame.width / 2 + dx * cos - dy * sin;
  const frameY = frame.height / 2 + dx * sin + dy * cos;
  return {
    x: (frameX - crop.x) / Math.max(1, crop.width),
    y: (frameY - crop.y) / Math.max(1, crop.height),
  };
}

export function brushRadiusInFrame(
  radius: number,
  crop: CropState,
  sourceWidth: number,
  sourceHeight: number,
  frameWidth: number,
): number {
  const shortest = Math.min(sourceWidth, sourceHeight);
  return (radius * shortest * frameWidth) / Math.max(1, crop.width);
}

export function normalizeMask(mask: LayerMask | null | undefined): LayerMask {
  return {
    strokes: mask?.strokes ?? [],
    autoSkin: mask?.autoSkin ?? false,
    regions: mask?.regions ?? [],
    gradient: mask?.gradient ?? null,
    inverted: mask?.inverted ?? false,
    feather: mask?.feather ?? 0,
    density: mask?.density ?? 1,
  };
}

export function defaultMaskGradient(kind: MaskGradientKind): MaskGradient {
  return kind === "linear"
    ? {
        kind,
        start: { x: 0.5, y: 0.2 },
        end: { x: 0.5, y: 0.8 },
      }
    : {
        kind,
        start: { x: 0.5, y: 0.5 },
        end: { x: 0.82, y: 0.5 },
      };
}

export function maskHasContent(mask: LayerMask | null | undefined): boolean {
  return Boolean(
    mask &&
      (mask.strokes.length > 0 ||
        mask.autoSkin ||
        mask.gradient ||
        (mask.regions?.length ?? 0) > 0 ||
        // A segmentation mask can be the only source: without this the UI would
        // report "全图生效" and hide actions that need a mask.
        mask.raster),
  );
}

export function maskSummary(mask: LayerMask | null | undefined): string {
  if (!maskHasContent(mask)) return "全图生效";
  const parts: string[] = [];
  if (mask?.autoSkin) parts.push("自动肤色");
  if (mask?.raster) parts.push("智能蒙版");
  const regions = mask?.regions ?? [];
  const faceScopes = regions.filter((region) => region.kind === "face").length;
  if (faceScopes > 0) {
    parts.push(faceScopes > 1 ? `人脸范围 ×${faceScopes}` : "人脸范围");
  }
  const measured = regions.filter((region) => region.kind !== "face");
  if (measured.length > 0) {
    parts.push(`五官区域 ${measured.map((region) => regionLabel(region.kind)).join("、")}`);
  }
  if (mask?.gradient) parts.push(mask.gradient.kind === "linear" ? "线性渐变" : "径向渐变");
  if (mask && mask.strokes.length > 0) parts.push(`${mask.strokes.length} 笔`);
  if (mask?.inverted) parts.push("已反转");
  const density = mask?.density ?? 1;
  if (density < 0.995) parts.push(`浓度 ${Math.round(density * 100)}%`);
  return parts.join(" + ");
}
