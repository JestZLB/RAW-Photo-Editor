export interface NormalizedCrop {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CropAspect = "free" | "1:1" | "4:5" | "3:4" | "16:9";

export const FULL_CROP: NormalizedCrop = { x: 0, y: 0, width: 1, height: 1 };

export const CROP_ASPECT_OPTIONS: Array<{ value: CropAspect; label: string }> = [
  { value: "free", label: "自由" },
  { value: "1:1", label: "1:1" },
  { value: "4:5", label: "4:5" },
  { value: "3:4", label: "3:4" },
  { value: "16:9", label: "16:9" },
];

export function aspectRatioOf(aspect: CropAspect): number | null {
  switch (aspect) {
    case "1:1":
      return 1;
    case "4:5":
      return 4 / 5;
    case "3:4":
      return 3 / 4;
    case "16:9":
      return 16 / 9;
    default:
      return null;
  }
}

export function clampNormalizedCrop(rect: NormalizedCrop): NormalizedCrop {
  const width = Math.min(1, Math.max(0.02, rect.width));
  const height = Math.min(1, Math.max(0.02, rect.height));
  return {
    width,
    height,
    x: Math.min(1 - width, Math.max(0, rect.x)),
    y: Math.min(1 - height, Math.max(0, rect.y)),
  };
}

/**
 * Largest centered crop that stays fully inside the rotated photo, normalized
 * to the rotated frame. `ratio` is the target pixel aspect (w / h); `null`
 * maximizes area. The result is capped by the source dimensions so rotating a
 * crop never enlarges the output.
 */
export function cropBounds(
  width: number,
  height: number,
  angle: number,
  ratio: number | null,
): NormalizedCrop {
  if (width <= 0 || height <= 0) return { ...FULL_CROP };
  const frame = rotatedFrameSize(width, height, angle);
  const radians = (Math.abs(angle) * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  let cropWidth: number;
  let cropHeight: number;

  if (ratio === null) {
    // Solve for the largest centered axis-aligned rectangle inside the
    // rotated source rectangle. Half-extents live in frame axes.
    const halfWidth = width / 2;
    const halfHeight = height / 2;
    const candidates = [];
    const determinant = cos * cos - sin * sin;
    if (Math.abs(determinant) > 1e-6) {
      candidates.push({
        x: (cos * halfWidth - sin * halfHeight) / determinant,
        y: (cos * halfHeight - sin * halfWidth) / determinant,
      });
    }
    if (sin > 1e-6) {
      candidates.push({ x: halfWidth / (2 * cos), y: halfWidth / (2 * sin) });
      candidates.push({ x: halfHeight / (2 * sin), y: halfHeight / (2 * cos) });
    }
    let best = { x: 0, y: 0 };
    for (const candidate of candidates) {
      const inside =
        candidate.x > 0 &&
        candidate.y > 0 &&
        cos * candidate.x + sin * candidate.y <= halfWidth + 1e-6 &&
        sin * candidate.x + cos * candidate.y <= halfHeight + 1e-6;
      if (inside && candidate.x * candidate.y > best.x * best.y) best = candidate;
    }
    cropWidth = Math.min(width, Math.max(1, best.x * 2));
    cropHeight = Math.min(height, Math.max(1, best.y * 2));
  } else {
    const safeRatio = ratio > 0 ? ratio : 1;
    let maxHeight = Math.min(
      width / (safeRatio * cos + sin),
      height / (safeRatio * sin + cos),
      height,
      width / safeRatio,
    );
    maxHeight = Math.max(1, maxHeight);
    cropWidth = maxHeight * safeRatio;
    cropHeight = maxHeight;
  }

  return {
    x: (frame.width - cropWidth) / 2 / frame.width,
    y: (frame.height - cropHeight) / 2 / frame.height,
    width: cropWidth / frame.width,
    height: cropHeight / frame.height,
  };
}

/** Shrink a draft so it stays inside `bounds`, keeping its center when possible. */
export function clampCropToBounds(
  rect: NormalizedCrop,
  bounds: NormalizedCrop,
  preserveRatio: boolean,
): NormalizedCrop {
  let width = Math.max(0.02, rect.width);
  let height = Math.max(0.02, rect.height);
  if (preserveRatio && rect.width > 0 && rect.height > 0) {
    const scale = Math.min(1, bounds.width / rect.width, bounds.height / rect.height);
    width *= scale;
    height *= scale;
  }
  width = Math.min(width, bounds.width);
  height = Math.min(height, bounds.height);
  const centerX = rect.x + rect.width / 2;
  const centerY = rect.y + rect.height / 2;
  return {
    width,
    height,
    x: Math.min(bounds.x + bounds.width - width, Math.max(bounds.x, centerX - width / 2)),
    y: Math.min(bounds.y + bounds.height - height, Math.max(bounds.y, centerY - height / 2)),
  };
}

/**
 * Keep the crop's physical size (not its share of a changing bounding box)
 * while the angle changes, then shrink it into the new safe area.
 */
export function fitCropForAngle(
  draft: NormalizedCrop,
  oldAngle: number,
  newAngle: number,
  width: number,
  height: number,
  ratio: number | null,
): NormalizedCrop {
  const oldFrame = rotatedFrameSize(width, height, oldAngle);
  const newFrame = rotatedFrameSize(width, height, newAngle);
  const bounds = cropBounds(width, height, newAngle, ratio);
  let nextWidth = draft.width * (oldFrame.width / newFrame.width);
  let nextHeight = draft.height * (oldFrame.height / newFrame.height);
  if (ratio !== null) {
    const scale = Math.min(1, bounds.width / nextWidth, bounds.height / nextHeight);
    nextWidth *= scale;
    nextHeight *= scale;
  } else {
    nextWidth = Math.min(nextWidth, bounds.width);
    nextHeight = Math.min(nextHeight, bounds.height);
  }
  const centerX = draft.x + draft.width / 2;
  const centerY = draft.y + draft.height / 2;
  return clampCropToBounds(
    { x: centerX - nextWidth / 2, y: centerY - nextHeight / 2, width: nextWidth, height: nextHeight },
    bounds,
    false,
  );
}

export function normalizedCropToPixels(
  draft: NormalizedCrop,
  base: { x: number; y: number; width: number; height: number },
): { x: number; y: number; width: number; height: number } {
  const width = Math.max(1, Math.round(draft.width * base.width));
  const height = Math.max(1, Math.round(draft.height * base.height));
  return {
    x: Math.max(0, Math.round(base.x + draft.x * base.width)),
    y: Math.max(0, Math.round(base.y + draft.y * base.height)),
    width: Math.min(base.width, width),
    height: Math.min(base.height, height),
  };
}

/** Bounding box of a frame rotated clockwise by `angle` degrees. */
export function rotatedFrameSize(
  width: number,
  height: number,
  angle: number,
): { width: number; height: number } {
  const radians = (angle * Math.PI) / 180;
  const sin = Math.abs(Math.sin(radians));
  const cos = Math.abs(Math.cos(radians));
  // The epsilon keeps exact right angles from rounding up by one pixel.
  return {
    width: Math.max(1, Math.ceil(width * cos + height * sin - 1e-6)),
    height: Math.max(1, Math.ceil(width * sin + height * cos - 1e-6)),
  };
}

/** Map a draft normalized over the rotated frame to absolute pixels. */
export function draftToRotatedPixels(
  draft: NormalizedCrop,
  frameWidth: number,
  frameHeight: number,
): { x: number; y: number; width: number; height: number } {
  const width = Math.min(frameWidth, Math.max(1, Math.round(draft.width * frameWidth)));
  const height = Math.min(frameHeight, Math.max(1, Math.round(draft.height * frameHeight)));
  return {
    x: Math.min(frameWidth - width, Math.max(0, Math.round(draft.x * frameWidth))),
    y: Math.min(frameHeight - height, Math.max(0, Math.round(draft.y * frameHeight))),
    width,
    height,
  };
}

/** Recover the normalized draft from a committed crop inside the rotated frame. */
export function pixelsToRotatedDraft(
  crop: { x: number; y: number; width: number; height: number },
  frameWidth: number,
  frameHeight: number,
): NormalizedCrop {
  return clampNormalizedCrop({
    x: crop.x / Math.max(1, frameWidth),
    y: crop.y / Math.max(1, frameHeight),
    width: crop.width / Math.max(1, frameWidth),
    height: crop.height / Math.max(1, frameHeight),
  });
}
