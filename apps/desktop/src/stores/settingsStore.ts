import { create } from "zustand";
import type {
  ExportBitDepth,
  ExportColorSpace,
  ExportFormat,
  ExportMetadata,
} from "../api/types";
import { EXPORT_SIZE_PRESETS, type ExportSize } from "../lib/export";

export const PREVIEW_MAX_DIMENSIONS = [1200, 1600, 2048] as const;
export type PreviewMaxDimension = (typeof PREVIEW_MAX_DIMENSIONS)[number];

export const THEMES = ["ink", "sand", "graphite", "pine", "indigo", "light"] as const;
export type ThemeId = (typeof THEMES)[number];

export const PORTRAIT_INTENSITIES = [55, 78, 100] as const;
export type PortraitIntensity = (typeof PORTRAIT_INTENSITIES)[number];

export const AI_DENOISE_ENGINES = ["photoHq", "onnx"] as const;
export type AiDenoiseEngine = (typeof AI_DENOISE_ENGINES)[number];

const EXPORT_FORMATS: readonly ExportFormat[] = ["jpeg", "png", "tiff", "webp"];
export const EXPORT_COLOR_SPACES: readonly ExportColorSpace[] = ["srgb", "displayP3", "source"];
export const EXPORT_METADATA_MODES: readonly ExportMetadata[] = ["keep", "privacy", "none"];
export const EXPORT_BIT_DEPTHS: readonly ExportBitDepth[] = ["eight", "sixteen"];

export const WATERMARK_POSITIONS = [
  "topLeft",
  "topRight",
  "bottomLeft",
  "bottomRight",
  "center",
] as const;
export type WatermarkPositionId = (typeof WATERMARK_POSITIONS)[number];
export const WATERMARK_KINDS = ["text", "image"] as const;
export type WatermarkKind = (typeof WATERMARK_KINDS)[number];
/** Data-URL budget for a stored logo (localStorage is the backing store). */
export const MAX_WATERMARK_DATA_URL = 2_000_000;

export interface AppSettings {
  previewMaxDimension: PreviewMaxDimension;
  exportFormat: ExportFormat;
  exportQuality: number;
  exportSize: ExportSize;
  exportColorSpace: ExportColorSpace;
  exportMetadata: ExportMetadata;
  exportBitDepth: ExportBitDepth;
  exportCustomWidth: number;
  exportCustomHeight: number;
  portraitIntensity: PortraitIntensity;
  theme: ThemeId;
  reduceMotion: boolean;
  /** Absolute `.onnx` path chosen in settings; only used by the desktop app. */
  aiDenoiseModelPath: string | null;
  /** User-supplied segmentation model; the app ships no weights. */
  segmentationModelPath: string | null;
  /** 0..100 blend applied when running the local ONNX denoiser. */
  aiDenoiseStrength: number;
  /** Preferred local denoise engine. */
  aiDenoiseEngine: AiDenoiseEngine;
  /** 0..100 blend for the bundled Photo-HQ-W4xEX 4x pass. */
  photoHqStrength: number;
  /** Optional export-only watermark (never touches the document or original). */
  watermarkEnabled: boolean;
  watermarkKind: WatermarkKind;
  watermarkText: string;
  /** Data URL for a logo; `null` until the user picks one. */
  watermarkImage: string | null;
  /** 0..100 blend for the watermark overlay. */
  watermarkOpacity: number;
  /** Watermark width as a percentage of the exported shortest edge. */
  watermarkScale: number;
  watermarkPosition: WatermarkPositionId;
}

export const DEFAULT_SETTINGS: AppSettings = {
  previewMaxDimension: 1600,
  exportFormat: "jpeg",
  exportQuality: 90,
  exportSize: "original",
  exportColorSpace: "srgb",
  exportMetadata: "keep",
  exportBitDepth: "eight",
  exportCustomWidth: 2048,
  exportCustomHeight: 2048,
  portraitIntensity: 78,
  theme: "ink",
  reduceMotion: false,
  aiDenoiseModelPath: null,
  segmentationModelPath: null,
  aiDenoiseStrength: 100,
  aiDenoiseEngine: "photoHq",
  photoHqStrength: 80,
  watermarkEnabled: false,
  watermarkKind: "text",
  watermarkText: "",
  watermarkImage: null,
  watermarkOpacity: 60,
  watermarkScale: 18,
  watermarkPosition: "bottomRight",
};

export const SETTINGS_STORAGE_KEY = "luma.settings.v1";

function includes<T extends string | number>(values: readonly T[], value: unknown): value is T {
  return values.includes(value as T);
}

export function sanitizeSettings(input: unknown): AppSettings {
  const raw = input && typeof input === "object" ? (input as Record<string, unknown>) : {};
  const quality =
    typeof raw.exportQuality === "number" && Number.isFinite(raw.exportQuality)
      ? Math.min(100, Math.max(1, Math.round(raw.exportQuality)))
      : DEFAULT_SETTINGS.exportQuality;
  const dimension = (value: unknown, fallback: number): number =>
    typeof value === "number" && Number.isFinite(value)
      ? Math.min(20000, Math.max(16, Math.round(value)))
      : fallback;
  const denoisePath =
    typeof raw.aiDenoiseModelPath === "string" && raw.aiDenoiseModelPath.trim()
      ? raw.aiDenoiseModelPath.trim()
      : null;
  const segmentationPath =
    typeof raw.segmentationModelPath === "string" && raw.segmentationModelPath.trim()
      ? raw.segmentationModelPath.trim()
      : null;
  const denoiseStrength =
    typeof raw.aiDenoiseStrength === "number" && Number.isFinite(raw.aiDenoiseStrength)
      ? Math.min(100, Math.max(0, Math.round(raw.aiDenoiseStrength)))
      : DEFAULT_SETTINGS.aiDenoiseStrength;
  const photoHqStrength =
    typeof raw.photoHqStrength === "number" && Number.isFinite(raw.photoHqStrength)
      ? Math.min(100, Math.max(0, Math.round(raw.photoHqStrength)))
      : DEFAULT_SETTINGS.photoHqStrength;
  const watermarkText =
    typeof raw.watermarkText === "string" ? raw.watermarkText.trim().slice(0, 120) : "";
  const watermarkImage =
    typeof raw.watermarkImage === "string" &&
    raw.watermarkImage.startsWith("data:image/") &&
    raw.watermarkImage.length <= MAX_WATERMARK_DATA_URL
      ? raw.watermarkImage
      : null;
  const watermarkOpacity =
    typeof raw.watermarkOpacity === "number" && Number.isFinite(raw.watermarkOpacity)
      ? Math.min(100, Math.max(5, Math.round(raw.watermarkOpacity)))
      : DEFAULT_SETTINGS.watermarkOpacity;
  const watermarkScale =
    typeof raw.watermarkScale === "number" && Number.isFinite(raw.watermarkScale)
      ? Math.min(60, Math.max(5, Math.round(raw.watermarkScale)))
      : DEFAULT_SETTINGS.watermarkScale;
  return {
    previewMaxDimension: includes(PREVIEW_MAX_DIMENSIONS, raw.previewMaxDimension)
      ? raw.previewMaxDimension
      : DEFAULT_SETTINGS.previewMaxDimension,
    exportFormat: includes(EXPORT_FORMATS, raw.exportFormat)
      ? raw.exportFormat
      : DEFAULT_SETTINGS.exportFormat,
    exportQuality: quality,
    exportSize: includes(EXPORT_SIZE_PRESETS, raw.exportSize)
      ? raw.exportSize
      : DEFAULT_SETTINGS.exportSize,
    exportColorSpace: includes(EXPORT_COLOR_SPACES, raw.exportColorSpace)
      ? raw.exportColorSpace
      : DEFAULT_SETTINGS.exportColorSpace,
    exportMetadata: includes(EXPORT_METADATA_MODES, raw.exportMetadata)
      ? raw.exportMetadata
      : DEFAULT_SETTINGS.exportMetadata,
    exportBitDepth: includes(EXPORT_BIT_DEPTHS, raw.exportBitDepth)
      ? raw.exportBitDepth
      : DEFAULT_SETTINGS.exportBitDepth,
    exportCustomWidth: dimension(raw.exportCustomWidth, DEFAULT_SETTINGS.exportCustomWidth),
    exportCustomHeight: dimension(raw.exportCustomHeight, DEFAULT_SETTINGS.exportCustomHeight),
    portraitIntensity: includes(PORTRAIT_INTENSITIES, raw.portraitIntensity)
      ? raw.portraitIntensity
      : DEFAULT_SETTINGS.portraitIntensity,
    theme: includes(THEMES, raw.theme) ? raw.theme : DEFAULT_SETTINGS.theme,
    reduceMotion:
      typeof raw.reduceMotion === "boolean"
        ? raw.reduceMotion
        : DEFAULT_SETTINGS.reduceMotion,
    aiDenoiseModelPath: denoisePath,
    segmentationModelPath: segmentationPath,
    aiDenoiseStrength: denoiseStrength,
    aiDenoiseEngine: includes(AI_DENOISE_ENGINES, raw.aiDenoiseEngine)
      ? raw.aiDenoiseEngine
      : DEFAULT_SETTINGS.aiDenoiseEngine,
    photoHqStrength: photoHqStrength,
    watermarkEnabled:
      typeof raw.watermarkEnabled === "boolean"
        ? raw.watermarkEnabled
        : DEFAULT_SETTINGS.watermarkEnabled,
    watermarkKind: includes(WATERMARK_KINDS, raw.watermarkKind)
      ? raw.watermarkKind
      : DEFAULT_SETTINGS.watermarkKind,
    watermarkText,
    watermarkImage,
    watermarkOpacity,
    watermarkScale,
    watermarkPosition: includes(WATERMARK_POSITIONS, raw.watermarkPosition)
      ? raw.watermarkPosition
      : DEFAULT_SETTINGS.watermarkPosition,
  };
}

function loadSettings(): AppSettings {
  if (typeof window === "undefined") return { ...DEFAULT_SETTINGS };
  try {
    const stored = window.localStorage.getItem(SETTINGS_STORAGE_KEY);
    return stored ? sanitizeSettings(JSON.parse(stored)) : { ...DEFAULT_SETTINGS };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function persistSettings(settings: AppSettings): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Storage can be unavailable (private mode/quota); defaults still apply.
  }
}

interface SettingsState extends AppSettings {
  update: (patch: Partial<AppSettings>) => void;
  reset: () => void;
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  ...loadSettings(),

  update: (patch) => {
    const next = sanitizeSettings({ ...get(), ...patch });
    set(next);
    persistSettings(next);
  },

  reset: () => {
    const next = { ...DEFAULT_SETTINGS };
    set(next);
    persistSettings(next);
  },
}));
