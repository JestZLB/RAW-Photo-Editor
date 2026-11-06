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

/** Default template; keeps the historical `name-luma` output. */
export const DEFAULT_EXPORT_NAME_TEMPLATE = "{name}-luma";

export interface ExportNameContext {
  name: string;
  collection?: string | null;
  width?: number;
  height?: number;
  /** 1-based position, used by batch export. */
  index?: number;
  /** Injected in tests; defaults to now. */
  date?: Date;
}

const pad2 = (value: number): string => String(value).padStart(2, "0");

/**
 * Render the user's filename template into a safe file stem (no extension).
 *
 * Unknown `{tokens}` are left untouched on purpose: seeing `{nmae}` in the
 * preview is a better hint than silently dropping it. The extension is always
 * appended by the caller, so a template can never produce `.jpg.jpg`.
 */
export function renderExportStem(template: string, context: ExportNameContext): string {
  const now = context.date ?? new Date();
  const values: Record<string, string> = {
    name: context.name,
    collection: context.collection ?? "",
    date: `${now.getFullYear()}${pad2(now.getMonth() + 1)}${pad2(now.getDate())}`,
    time: `${pad2(now.getHours())}${pad2(now.getMinutes())}${pad2(now.getSeconds())}`,
    index: context.index === undefined ? "" : pad2(context.index),
    width: context.width === undefined ? "" : String(context.width),
    height: context.height === undefined ? "" : String(context.height),
  };
  const source = template.trim() || DEFAULT_EXPORT_NAME_TEMPLATE;
  const replaced = source.replace(/\{(\w+)\}/g, (match, key: string) =>
    key in values ? values[key] : match,
  );
  // Windows rejects names that end with a dot or space; "..." must not become
  // a file called "...".
  const trimmed = safeFileStem(replaced).replace(/^[.\s]+|[.\s]+$/g, "");
  // 96 leaves room for a `-2` duplicate suffix and a long extension.
  const stem = trimmed.slice(0, 96);
  return stem || "luma-photo";
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
