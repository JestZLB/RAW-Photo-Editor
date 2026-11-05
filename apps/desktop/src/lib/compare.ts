export interface SourcePlacement {
  width: number;
  height: number;
  left: number;
  top: number;
}

/**
 * Places the full source image inside the visible (cropped) preview frame so a
 * split comparison stays aligned after non-destructive cropping.
 */
export function sourcePlacement(
  sourceWidth: number,
  sourceHeight: number,
  crop: { x: number; y: number; width: number; height: number },
  displayWidth: number,
  displayHeight: number,
): SourcePlacement {
  const scaleX = displayWidth / Math.max(1, crop.width);
  const scaleY = displayHeight / Math.max(1, crop.height);
  return {
    width: sourceWidth * scaleX,
    height: sourceHeight * scaleY,
    left: -crop.x * scaleX,
    top: -crop.y * scaleY,
  };
}
