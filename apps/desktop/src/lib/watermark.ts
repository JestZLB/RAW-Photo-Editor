import type { WatermarkOptions } from "../api/types";
import type { AppSettings } from "../stores/settingsStore";

const TEXT_FONT_PX = 96;
const TEXT_PADDING = 32;
const MAX_TEXT_WIDTH = 2400;

export function hasWatermarkContent(settings: AppSettings): boolean {
  return settings.watermarkKind === "text"
    ? settings.watermarkText.trim().length > 0
    : Boolean(settings.watermarkImage);
}

/** Build the IPC payload; returns null when the watermark is off or empty. */
export async function buildWatermarkOptions(
  settings: AppSettings,
): Promise<WatermarkOptions | null> {
  if (!settings.watermarkEnabled) return null;
  const imageBase64 =
    settings.watermarkKind === "image"
      ? settings.watermarkImage
      : rasterizeWatermarkText(settings.watermarkText);
  if (!imageBase64) return null;
  return {
    imageBase64,
    opacity: settings.watermarkOpacity / 100,
    scale: settings.watermarkScale / 100,
    margin: 0.04,
    position: settings.watermarkPosition,
  };
}

/**
 * Draw the studio name into a transparent PNG. The backend only scales and
 * blends pixels, which keeps font handling (and CJK shaping) in the browser.
 */
export function rasterizeWatermarkText(text: string): string | null {
  const value = text.trim();
  if (!value || typeof document === "undefined") return null;
  const fontFor = (size: number): string =>
    `600 ${size}px "Microsoft YaHei", "PingFang SC", "Noto Sans SC", system-ui, sans-serif`;
  const measure = document.createElement("canvas");
  const ctx = measure.getContext("2d");
  if (!ctx) return null;
  let fontSize = TEXT_FONT_PX;
  ctx.font = fontFor(fontSize);
  let width = ctx.measureText(value).width;
  const maxTextWidth = MAX_TEXT_WIDTH - TEXT_PADDING * 2;
  if (width > maxTextWidth) {
    fontSize = Math.max(28, Math.floor((fontSize * maxTextWidth) / width));
    ctx.font = fontFor(fontSize);
    width = ctx.measureText(value).width;
  }
  const canvasWidth = Math.ceil(width + TEXT_PADDING * 2);
  const canvasHeight = Math.ceil(fontSize * 1.6);
  measure.width = canvasWidth;
  measure.height = canvasHeight;
  const draw = measure.getContext("2d");
  if (!draw) return null;
  draw.font = fontFor(fontSize);
  draw.textAlign = "center";
  draw.textBaseline = "middle";
  draw.shadowColor = "rgba(0, 0, 0, 0.35)";
  draw.shadowBlur = Math.max(2, fontSize * 0.12);
  draw.shadowOffsetY = Math.max(1, fontSize * 0.04);
  draw.fillStyle = "#ffffff";
  draw.fillText(value, canvasWidth / 2, canvasHeight / 2);
  return measure.toDataURL("image/png");
}
