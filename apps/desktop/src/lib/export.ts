import type { ExportFormat } from "../api/types";

export const EXPORT_SIZE_PRESETS = ["original", "1:1", "4:5", "3:4", "16:9", "custom"] as const;
export type ExportSize = (typeof EXPORT_SIZE_PRESETS)[number];

export const EXPORT_SIZE_OPTIONS: Array<{ value: ExportSize; label: string; hint: string }> = [
  { value: "original", label: "原图", hint: "当前裁剪尺寸" },
  { value: "1:1", label: "1:1", hint: "方形" },
  { value: "4:5", label: "4:5", hint: "社交人像" },
  { value: "3:4", label: "3:4", hint: "竖版" },
  { value: "16:9", label: "16:9", hint: "宽屏" },
  { value: "custom", label: "自定义", hint: "指定宽高" },
];

function ratioFor(size: ExportSize): number | null {
  switch (size) {
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

/** Safe file stem shared by single and batch export. */
export function safeFileStem(name: string): string {
  return name
    .replace(/[\\/:*?"<>|]+/g, "-")
    .replace(/\s+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
}

export function extensionForFormat(format: ExportFormat): string {
  return format === "jpeg" ? "jpg" : format;
}

/** Largest centered crop of the requested ratio at native pixel size. */
export function outputDimensions(
  width: number,
  height: number,
  size: ExportSize,
  customWidth = width,
  customHeight = height,
): { width: number; height: number } {
  if (width <= 0 || height <= 0) return { width: 1, height: 1 };
  if (size === "custom") {
    return {
      width: Math.max(1, Math.round(customWidth)),
      height: Math.max(1, Math.round(customHeight)),
    };
  }
  const ratio = ratioFor(size);
  if (ratio === null) return { width, height };
  let outWidth = Math.min(width, Math.round(height * ratio));
  let outHeight = Math.round(outWidth / ratio);
  if (outHeight > height) {
    outHeight = height;
    outWidth = Math.round(outHeight * ratio);
  }
  return { width: Math.max(1, outWidth), height: Math.max(1, outHeight) };
}
