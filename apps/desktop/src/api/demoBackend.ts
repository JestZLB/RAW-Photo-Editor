import { clamp, clamp01, smoothstep } from "../lib/clamp";
import {
  base64ToBlob,
  downloadDataUrl,
  extensionOf,
  MAX_LUT_BYTES,
  mimeForFileName,
  readAsDataUrl,
} from "../lib/file";
import {
  autoToneFromLumaHistogram,
  histogramFromRgba,
  lumaHistogramFromRgba,
} from "../lib/histogram";
import { applyHsl } from "../lib/hsl";
import { applyToneLut, buildToneLut, usesToneCurve } from "../lib/colorAdjust";
import { parseCube, sampleCube, type CubeLut } from "../lib/lut";
import { DEFAULT_BLUR, DEFAULT_COLOR, DEFAULT_PORTRAIT } from "../lib/params";
import { neutralWhiteBalance } from "../lib/whiteBalance";
import type { EditorApi } from "./client";
import { apiError } from "./errors";
import type {
  AddLayerRequest,
  AiCapabilityRequest,
  AiDenoiseCapability,
  AiDenoiseRequest,
  AiJobInfo,
  AiJobStatus,
  PhotoHqDenoiseRequest,
  AppInfo,
  ApplyAiResultRequest,
  BlendMode,
  CacheClearResult,
  CollectionSummary,
  ColorParams,
  ClearRasterMaskRequest,
  CommitChangesRequest,
  CreateProjectRequest,
  CreateVersionRequest,
  CropState,
  DeleteLayerRequest,
  DocumentState,
  DuplicateLayerRequest,
  ExportFormat,
  ExportImageRequest,
  ExportResult,
  ImportPhotoBytesRequest,
  ImportPhotoRequest,
  ImportLutRequest,
  JobIdRequest,
  LayerState,
  LutAssetResult,
  MoveLayerRequest,
  AutoToneResult,
  CancelExportRequest,
  ExportProgress,
  OpenProjectRequest,
  PickWhiteBalanceRequest,
  PortraitAnalysisResult,
  PortraitRegionsResult,
  PortraitParams,
  PreviewResult,
  ProjectRef,
  SegmentationCapability,
  ProjectSummary,
  ProviderCapabilities,
  ProviderConfigInput,
  ProviderIdRequest,
  ProviderInfo,
  ProviderTestResult,
  RenameProjectRequest,
  RenderPreviewRequest,
  SetCropRequest,
  SetProviderCredentialRequest,
  LayerMask,
  SourceAssetState,
  SubmitAiJobRequest,
  UpdateLayerParamsRequest,
  UpdateLayerRequest,
  SetLayerMaskRequest,
  SyncColorAdjustmentsRequest,
  SyncColorAdjustmentsResult,
  VersionIdRequest,
  VersionPreviewRequest,
  VersionSummary,
  WhiteBalanceResult,
} from "./types";

const MOCK_PROVIDER_ID = "mock-demo-provider";
const MAX_IMPORT_BYTES = 32 * 1024 * 1024;
const PREVIEW_MAX_DIMENSION = 1600;

interface HistorySnapshot {
  layers: LayerState[];
  crop: CropState;
}

interface HistoryEntry extends HistorySnapshot {
  label: string;
}

interface DemoVersion {
  summary: VersionSummary;
  snapshot: DocumentState;
}

interface DemoProject {
  doc: DocumentState;
  /** Original pixels as a data URL: exports must stay full resolution. */
  sourceUrl: string;
  undoStack: HistoryEntry[];
  redoStack: HistoryEntry[];
  lastCommitted: HistorySnapshot;
  /** Named snapshots, newest first; the browser demo keeps them in memory. */
  versions: DemoVersion[];
}

const BLEND_TO_COMPOSITE: Record<BlendMode, GlobalCompositeOperation> = {
  normal: "source-over",
  multiply: "multiply",
  screen: "screen",
  overlay: "overlay",
  softLight: "soft-light",
};

function nowIso(): string {
  return new Date().toISOString();
}

function uid(prefix: string): string {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi && typeof cryptoApi.randomUUID === "function") {
    return `${prefix}-${cryptoApi.randomUUID()}`;
  }
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

function cloneColor(color: ColorParams | null): ColorParams | null {
  return color ? { ...color } : null;
}

function clonePortrait(portrait: PortraitParams | null): PortraitParams | null {
  return portrait ? { ...portrait } : null;
}

function cloneMask(mask: LayerMask | null): LayerMask | null {
  return mask
    ? {
        strokes: mask.strokes.map((stroke) => ({
          ...stroke,
          points: stroke.points.map((point) => ({ ...point })),
        })),
        autoSkin: mask.autoSkin,
        gradient: mask.gradient
          ? {
              ...mask.gradient,
              start: { ...mask.gradient.start },
              end: { ...mask.gradient.end },
            }
          : null,
        raster: mask.raster ? { ...mask.raster } : null,
        inverted: mask.inverted ?? false,
        feather: mask.feather ?? 0,
        density: mask.density ?? 1,
      }
    : null;
}

function cloneLayers(layers: LayerState[]): LayerState[] {
  return layers.map((layer) => ({
    ...layer,
    color: cloneColor(layer.color),
    portrait: clonePortrait(layer.portrait),
    mask: cloneMask(layer.mask),
  }));
}

function cloneDocument(doc: DocumentState): DocumentState {
  return {
    ...doc,
    canvas: { ...doc.canvas },
    source: doc.source ? { ...doc.source } : null,
    layers: cloneLayers(doc.layers),
    history: [...doc.history],
  };
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.decoding = "async";
    image.onload = () => resolve(image);
    image.onerror = () => reject(apiError("INVALID_IMAGE", "Image decode failed", "这张照片无法读取，请换一张 JPG、PNG 或 WebP 图片。"));
    image.src = src;
  });
}

function createCanvas(width: number, height: number): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width));
  canvas.height = Math.max(1, Math.round(height));
  return canvas;
}

function drawImageCover(
  ctx: CanvasRenderingContext2D,
  image: HTMLImageElement,
  width: number,
  height: number,
): void {
  const scale = Math.max(width / image.naturalWidth, height / image.naturalHeight);
  const drawWidth = image.naturalWidth * scale;
  const drawHeight = image.naturalHeight * scale;
  ctx.drawImage(image, (width - drawWidth) / 2, (height - drawHeight) / 2, drawWidth, drawHeight);
}

function coverCanvas(
  source: HTMLCanvasElement,
  width: number,
  height: number,
): HTMLCanvasElement {
  const out = createCanvas(width, height);
  const ctx = context2d(out);
  const scale = Math.max(width / source.width, height / source.height);
  const drawWidth = source.width * scale;
  const drawHeight = source.height * scale;
  ctx.drawImage(source, (width - drawWidth) / 2, (height - drawHeight) / 2, drawWidth, drawHeight);
  return out;
}

function context2d(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw apiError("CANVAS_UNAVAILABLE", "2D context unavailable", "当前浏览器无法进行本地渲染，请使用最新版桌面浏览器。");
  return ctx;
}

function dataUrlBytes(dataUrl: string): number {
  const comma = dataUrl.indexOf(",");
  const base64 = comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor((base64.length * 3) / 4) - padding);
}

function clampedCrop(crop: CropState, canvas: { width: number; height: number }): CropState {
  if (canvas.width <= 0 || canvas.height <= 0) {
    return { x: 0, y: 0, width: 0, height: 0 };
  }
  const width = Math.min(canvas.width, Math.max(1, Math.round(crop.width)));
  const height = Math.min(canvas.height, Math.max(1, Math.round(crop.height)));
  return {
    x: Math.min(canvas.width - width, Math.max(0, Math.round(crop.x))),
    y: Math.min(canvas.height - height, Math.max(0, Math.round(crop.y))),
    width,
    height,
  };
}

function displayNameOf(fileName: string): string {
  const stem = fileName.replace(/\.[^.]+$/, "").replace(/[-_]+/g, " ").trim();
  return stem.length > 0 ? stem.replace(/\b\w/g, (letter) => letter.toUpperCase()) : "未命名照片";
}

function skinMaskAt(r: number, g: number, b: number): number {
  if (r < 45 || g < 30 || b < 16) return 0;
  const cb = 128 - 0.168736 * r - 0.331264 * g + 0.5 * b;
  const cr = 128 + 0.5 * r - 0.418688 * g - 0.081312 * b;
  if (cb < 76 || cb > 133 || cr < 132 || cr > 180) return 0;
  if (r < g || r < b) return 0;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const saturation = max > 0 ? (max - min) / max : 0;
  if (saturation > 0.68) return 0.35;
  return 1;
}

function rasterizeMask(
  mask: LayerMask,
  source: HTMLCanvasElement,
  sourceWidth: number,
  sourceHeight: number,
  crop: CropState,
  width: number,
  height: number,
): Float32Array | null {
  const regions = mask.regions ?? [];
  if (
    (mask.strokes.length === 0 &&
      !mask.autoSkin &&
      !mask.gradient &&
      regions.length === 0) ||
    width <= 0 ||
    height <= 0
  ) {
    return null;
  }
  const maskCanvas = createCanvas(width, height);
  const ctx = context2d(maskCanvas);
  if (mask.autoSkin) {
    const pixels = context2d(source).getImageData(0, 0, width, height).data;
    const auto = ctx.createImageData(width, height);
    for (let index = 0; index < width * height; index += 1) {
      const base = index * 4;
      const coverage = skinMaskAt(pixels[base], pixels[base + 1], pixels[base + 2]);
      auto.data[base] = 255;
      auto.data[base + 1] = 255;
      auto.data[base + 2] = 255;
      auto.data[base + 3] = Math.round(coverage * 255);
    }
    const autoCanvas = createCanvas(width, height);
    context2d(autoCanvas).putImageData(auto, 0, 0);
    const blur = Math.min(12, Math.max(1, Math.round(Math.min(width, height) / 400)));
    ctx.save();
    ctx.filter = `blur(${blur}px)`;
    ctx.drawImage(autoCanvas, 0, 0);
    ctx.restore();
  }
  const scaleX = width / Math.max(1, crop.width);
  const scaleY = height / Math.max(1, crop.height);
  if (mask.gradient) {
    const startX = (mask.gradient.start.x * sourceWidth - crop.x) * scaleX;
    const startY = (mask.gradient.start.y * sourceHeight - crop.y) * scaleY;
    const endX = (mask.gradient.end.x * sourceWidth - crop.x) * scaleX;
    const endY = (mask.gradient.end.y * sourceHeight - crop.y) * scaleY;
    const radius = Math.hypot(endX - startX, endY - startY);
    if (radius > 0.5) {
      const gradient =
        mask.gradient.kind === "linear"
          ? ctx.createLinearGradient(startX, startY, endX, endY)
          : ctx.createRadialGradient(startX, startY, 0, startX, startY, radius);
      gradient.addColorStop(0, "rgba(255,255,255,1)");
      gradient.addColorStop(1, "rgba(255,255,255,0)");
      ctx.save();
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, width, height);
      ctx.restore();
    }
  }
  const shortest = Math.min(sourceWidth, sourceHeight);
  for (const stroke of mask.strokes) {
    if (stroke.points.length === 0) continue;
    const radius = Math.max(0.6, stroke.radius * shortest * scaleX);
    const blur = radius * Math.max(0, Math.min(1, stroke.feather));
    ctx.save();
    ctx.globalCompositeOperation = stroke.erase ? "destination-out" : "source-over";
    if (blur > 0.3) ctx.filter = `blur(${blur.toFixed(2)}px)`;
    ctx.strokeStyle = "rgba(255,255,255,1)";
    ctx.fillStyle = "rgba(255,255,255,1)";
    ctx.lineWidth = radius * 2;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    const points = stroke.points.map((point) => ({
      x: (point.x * sourceWidth - crop.x) * scaleX,
      y: (point.y * sourceHeight - crop.y) * scaleY,
    }));
    if (points.length === 1) {
      ctx.beginPath();
      ctx.arc(points[0].x, points[0].y, radius, 0, Math.PI * 2);
      ctx.fill();
    } else {
      ctx.beginPath();
      ctx.moveTo(points[0].x, points[0].y);
      for (const point of points.slice(1)) ctx.lineTo(point.x, point.y);
      ctx.stroke();
    }
    ctx.restore();
  }
  for (const region of regions) {
    const centerX = (region.centerX * sourceWidth - crop.x) * scaleX;
    const centerY = (region.centerY * sourceHeight - crop.y) * scaleY;
    const radiusX = Math.max(0.6, region.radiusX * sourceWidth * scaleX);
    const radiusY = Math.max(0.6, region.radiusY * sourceHeight * scaleY);
    const gradient = ctx.createRadialGradient(0, 0, 0, 0, 0, 1);
    const inner = Math.max(0, Math.min(0.95, 1 - region.feather));
    gradient.addColorStop(0, `rgba(255,255,255,${region.strength})`);
    gradient.addColorStop(inner, `rgba(255,255,255,${region.strength})`);
    gradient.addColorStop(1, "rgba(255,255,255,0)");
    ctx.save();
    ctx.globalCompositeOperation = "source-over";
    ctx.translate(centerX, centerY);
    ctx.rotate(region.angle);
    ctx.scale(radiusX, radiusY);
    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.arc(0, 0, 1, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
  let data = ctx.getImageData(0, 0, width, height).data;
  const feather = clamp01(mask.feather ?? 0);
  if (feather > 0) {
    const radius = Math.max(1, Math.round(Math.min(width, height) * 0.08 * feather));
    const blurred = createCanvas(width, height);
    const blurCtx = context2d(blurred);
    blurCtx.filter = `blur(${radius}px)`;
    blurCtx.drawImage(maskCanvas, 0, 0);
    data = blurCtx.getImageData(0, 0, width, height).data;
  }
  const inverted = mask.inverted === true;
  const density = clamp01(mask.density ?? 1);
  const out = new Float32Array(width * height);
  for (let index = 0; index < out.length; index += 1) {
    let value = data[index * 4 + 3] / 255;
    if (inverted) value = 1 - value;
    out[index] = value * density;
  }
  return out;
}

function applyColorToCanvas(
  canvas: HTMLCanvasElement,
  params: ColorParams,
  opacity: number,
  mask?: Float32Array | null,
  cube?: CubeLut | null,
): void {
  const ctx = context2d(canvas);
  const width = canvas.width;
  const height = canvas.height;
  const original = ctx.getImageData(0, 0, width, height);
  const data = original.data;

  let blurred: ImageData | null = null;
  if (Math.abs(params.clarity) > 0.4 || params.sharpness > 0.4) {
    const blurCanvas = createCanvas(width, height);
    const blurCtx = context2d(blurCanvas);
    const radius = Math.max(1, Math.min(width, height) * 0.0022);
    blurCtx.filter = `blur(${radius.toFixed(2)}px)`;
    blurCtx.drawImage(canvas, 0, 0);
    blurred = blurCtx.getImageData(0, 0, width, height);
  }

  const exposure = 2 ** params.exposure;
  const contrast = 1 + (params.contrast / 100) * 0.72;
  const temperature = params.temperature / 100;
  const tint = params.tint / 100;
  const saturation = params.saturation / 100;
  const vibrance = params.vibrance / 100;
  const hueShift = params.hue ?? 0;
  const hslSaturation = params.hslSaturation ?? 0;
  const hslLightness = params.hslLightness ?? 0;
  const toneParams: ColorParams = {
    ...DEFAULT_COLOR,
    ...params,
    curveX: params.curveX ?? DEFAULT_COLOR.curveX,
    curveY: params.curveY ?? DEFAULT_COLOR.curveY,
  };
  const toneLut = usesToneCurve(toneParams) ? buildToneLut(toneParams) : null;
  const highlights = params.highlights / 100;
  const shadows = params.shadows / 100;
  const whites = params.whites / 100;
  const blacks = params.blacks / 100;
  const clarity = params.clarity / 100;
  const sharpness = params.sharpness / 100;
  const mix = clamp01(opacity);

  for (let index = 0; index < data.length; index += 4) {
    const r0 = data[index] / 255;
    const g0 = data[index + 1] / 255;
    const b0 = data[index + 2] / 255;

    let r = r0 * exposure;
    let g = g0 * exposure;
    let b = b0 * exposure;

    r *= (1 + temperature * 0.25) * (1 + tint * 0.05);
    g *= 1 - tint * 0.15;
    b *= (1 - temperature * 0.25) * (1 + tint * 0.05);

    r = (r - 0.5) * contrast + 0.5;
    g = (g - 0.5) * contrast + 0.5;
    b = (b - 0.5) * contrast + 0.5;

    r = clamp01(r);
    g = clamp01(g);
    b = clamp01(b);

    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const highlightWeight = smoothstep(0.45, 1, luma);
    const shadowWeight = 1 - smoothstep(0.05, 0.58, luma);
    const whiteWeight = smoothstep(0.72, 1, luma);
    const blackWeight = 1 - smoothstep(0, 0.3, luma);
    const lift = highlights * 0.24 * highlightWeight + shadows * 0.2 * shadowWeight + whites * 0.17 * whiteWeight + blacks * 0.17 * blackWeight;
    r = clamp01(r + lift);
    g = clamp01(g + lift);
    b = clamp01(b + lift);

    if (hueShift !== 0 || hslSaturation !== 0 || hslLightness !== 0) {
      [r, g, b] = applyHsl(r, g, b, hueShift, hslSaturation, hslLightness);
    }
    if (toneLut) {
      r = applyToneLut(r, toneLut);
      g = applyToneLut(g, toneLut);
      b = applyToneLut(b, toneLut);
    }

    luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    const saturationFactor = 1 + saturation * 0.9;
    r = clamp01(luma + (r - luma) * saturationFactor);
    g = clamp01(luma + (g - luma) * saturationFactor);
    b = clamp01(luma + (b - luma) * saturationFactor);

    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const chroma = max - min;
    const vibranceFactor = 1 + vibrance * 0.65 * (1 - chroma);
    luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    r = clamp01(luma + (r - luma) * vibranceFactor);
    g = clamp01(luma + (g - luma) * vibranceFactor);
    b = clamp01(luma + (b - luma) * vibranceFactor);

    if (blurred) {
      const br = blurred.data[index] / 255;
      const bg = blurred.data[index + 1] / 255;
      const bb = blurred.data[index + 2] / 255;
      const blurLuma = 0.2126 * br + 0.7152 * bg + 0.0722 * bb;
      const localLuma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      const detail = localLuma - blurLuma;
      const clarityBump = detail * clarity * 0.5;
      r = clamp01(r + clarityBump + (r - br) * sharpness * 0.75);
      g = clamp01(g + clarityBump + (g - bg) * sharpness * 0.75);
      b = clamp01(b + clarityBump + (b - bb) * sharpness * 0.75);
    }

    if (cube) {
      [r, g, b] = sampleCube(cube, r, g, b, params.lutStrength ?? 1);
    }

    const coverage = mask ? mask[index / 4] : 1;
    const pixelMix = mix * coverage;
    data[index] = Math.round((r0 + (r - r0) * pixelMix) * 255);
    data[index + 1] = Math.round((g0 + (g - g0) * pixelMix) * 255);
    data[index + 2] = Math.round((b0 + (b - b0) * pixelMix) * 255);
  }

  ctx.putImageData(original, 0, 0);
}

/**
 * Approximate the desktop blur pass: draw the canvas through a CSS blur filter,
 * then blend it back only where the mask covers. A missing mask blurs the whole
 * frame, matching the Rust renderer.
 */
function applyBlurToCanvas(
  canvas: HTMLCanvasElement,
  radius: number,
  opacity: number,
  mask: Float32Array | null,
): void {
  const width = canvas.width;
  const height = canvas.height;
  const shortest = Math.min(width, height);
  const pixels = Math.max(0, Math.min(0.08, radius)) * shortest;
  const alpha = clamp01(opacity);
  if (pixels < 0.5 || alpha <= 0) return;

  const blurred = createCanvas(width, height);
  const blurCtx = context2d(blurred);
  blurCtx.filter = `blur(${pixels.toFixed(2)}px)`;
  blurCtx.drawImage(canvas, 0, 0);
  blurCtx.filter = "none";

  if (!mask) {
    const ctx = context2d(canvas);
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.drawImage(blurred, 0, 0);
    ctx.restore();
    return;
  }

  const soft = context2d(blurred).getImageData(0, 0, width, height);
  const ctx = context2d(canvas);
  const sharp = ctx.getImageData(0, 0, width, height);
  for (let index = 0; index < mask.length; index += 1) {
    const coverage = clamp01(mask[index]) * alpha;
    if (coverage <= 0) continue;
    const at = index * 4;
    for (let channel = 0; channel < 3; channel += 1) {
      sharp.data[at + channel] = Math.round(
        sharp.data[at + channel] * (1 - coverage) + soft.data[at + channel] * coverage,
      );
    }
  }
  ctx.putImageData(sharp, 0, 0);
}

function applyPortraitToCanvas(
  canvas: HTMLCanvasElement,
  params: PortraitParams,
  opacity: number,
  mask?: Float32Array | null,
): void {
  const intensity = clamp01(params.intensity / 100);
  const smooth = clamp01(params.smooth / 100);
  const bodySmooth = clamp01(params.bodySmooth / 100);
  const textureKeep = 1 - clamp01(params.texture / 100) * 0.58;
  const smoothAmount = clamp((smooth * 0.9 + bodySmooth * 0.32) * intensity * textureKeep, 0, 0.92);
  const evenAmount = clamp((params.evenSkin / 100) * 0.5 * intensity, 0, 0.8);
  const brightenAmount = clamp((params.brighten / 100) * 0.24 * intensity, 0, 0.5);
  if (smoothAmount < 0.004 && evenAmount < 0.004 && brightenAmount < 0.004) return;

  const ctx = context2d(canvas);
  const width = canvas.width;
  const height = canvas.height;
  const original = ctx.getImageData(0, 0, width, height);
  const data = original.data;

  const blurCanvas = createCanvas(width, height);
  const blurCtx = context2d(blurCanvas);
  const radius = Math.max(1.6, Math.min(width, height) * 0.0065 * (0.7 + smooth));
  blurCtx.filter = `blur(${radius.toFixed(2)}px)`;
  blurCtx.drawImage(canvas, 0, 0);
  const blurred = blurCtx.getImageData(0, 0, width, height);
  const mix = clamp01(opacity);

  for (let index = 0; index < data.length; index += 4) {
    const r0 = data[index];
    const g0 = data[index + 1];
    const b0 = data[index + 2];
    const skin = skinMaskAt(r0, g0, b0);
    if (skin <= 0) continue;

    const br = blurred.data[index];
    const bg = blurred.data[index + 1];
    const bb = blurred.data[index + 2];

    let r = r0 + (br - r0) * smoothAmount * skin;
    let g = g0 + (bg - g0) * smoothAmount * skin;
    let b = b0 + (bb - b0) * smoothAmount * skin;

    r += (br - r) * evenAmount * skin;
    g += (bg - g) * evenAmount * skin;
    b += (bb - b) * evenAmount * skin;

    if (brightenAmount > 0) {
      r += (255 - r) * brightenAmount * skin;
      g += (255 - g) * brightenAmount * skin;
      b += (255 - b) * brightenAmount * skin;
    }

    const pixelMix = mix * (mask ? mask[index / 4] : 1);
    data[index] = Math.round(r0 + (r - r0) * pixelMix);
    data[index + 1] = Math.round(g0 + (g - g0) * pixelMix);
    data[index + 2] = Math.round(b0 + (b - b0) * pixelMix);
  }

  ctx.putImageData(original, 0, 0);
}

export class DemoBackend implements EditorApi {
  readonly mode = "demo" as const;

  private projects = new Map<string, DemoProject>();
  private imageCache = new Map<string, Promise<HTMLImageElement>>();
  /** Imported cube LUTs, keyed by the asset id stored in `ColorParams`. */
  private luts = new Map<string, CubeLut>();
  private providers: ProviderInfo[] = [
    {
      id: MOCK_PROVIDER_ID,
      name: "本地演示服务",
      kind: "mock",
      baseUrl: null,
      model: "luma-demo-v1",
      hasCredential: true,
      capabilities: {
        supportsMask: false,
        supportsReferenceImages: false,
        supportsMultipleImages: false,
        returnsImage: true,
        supportsStructuredParameters: false,
        supportsCancellation: true,
        maxWidth: 1400,
        maxHeight: 1400,
        maxInputBytes: null,
      },
      createdAt: nowIso(),
    },
  ];
  private jobs: AiJobInfo[] = [];
  private jobTimers = new Map<string, number>();
  private jobListeners = new Set<(job: AiJobInfo) => void>();
  private exportListeners = new Set<(progress: ExportProgress) => void>();
  private cancelledExports = new Set<string>();

  private emitExport(
    jobId: string | undefined,
    phase: string,
    percent: number,
    message: string,
  ): void {
    if (!jobId) return;
    const progress: ExportProgress = { jobId, phase, percent, message };
    for (const listener of this.exportListeners) listener(progress);
  }

  private throwIfExportCancelled(jobId: string | undefined): void {
    if (jobId && this.cancelledExports.has(jobId)) {
      throw apiError("EXPORT_CANCELLED", "Export cancelled", "导出已取消。");
    }
  }

  async getAppInfo(): Promise<AppInfo> {
    return {
      version: "0.1.0-demo",
      backend: "cpu",
      gpuAvailable: false,
      rawAvailable: false,
      faceModelAvailable: false,
      workspaceDir: "浏览器内存会话",
      supportedImageFormats: ["jpg", "jpeg", "png", "tif", "tiff", "webp"],
    };
  }

  async listProjects(): Promise<ProjectSummary[]> {
    return [...this.projects.values()]
      .map((project) => this.summaryOf(project))
      .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  }

  async listCollections(): Promise<CollectionSummary[]> {
    const collections = new Map<string, CollectionSummary>();
    for (const project of await this.listProjects()) {
      const existing = collections.get(project.collectionId);
      if (existing) {
        existing.photoCount += 1;
        continue;
      }
      collections.set(project.collectionId, {
        id: project.collectionId,
        name: project.collectionName,
        photoCount: 1,
        updatedAt: project.updatedAt,
        coverThumbnailPath: project.thumbnailPath,
        latestProjectId: project.id,
      });
    }
    return [...collections.values()];
  }

  async createProject(req: CreateProjectRequest): Promise<DocumentState> {
    const id = uid("project");
    const timestamp = nowIso();
    const doc: DocumentState = {
      id,
      name: req.name.trim() || "未命名项目",
      collectionId: id,
      collectionName: req.name.trim() || "未命名项目",
      schemaVersion: 2,
      revision: 1,
      createdAt: timestamp,
      updatedAt: timestamp,
      canvas: { width: 0, height: 0, workingColorSpace: "linear-srgb" },
      crop: { x: 0, y: 0, width: 0, height: 0 },
      source: null,
      layers: [],
      canUndo: false,
      canRedo: false,
      history: [],
    };
    // A project without an image cannot be rendered in the demo; it is not
    // stored in the recent list until a photo is imported.
    return doc;
  }

  async openProject(req: OpenProjectRequest): Promise<DocumentState> {
    await delay(80);
    const project = this.projects.get(req.projectId);
    if (!project) {
      throw apiError("PROJECT_NOT_FOUND", "Project not found", "找不到这个项目，它可能已不在当前演示会话中。");
    }
    return cloneDocument(project.doc);
  }

  async renameProject(req: RenameProjectRequest): Promise<ProjectSummary> {
    await delay(40);
    const project = this.requireProject(req.projectId);
    const name = req.name.trim();
    if (!name) {
      throw apiError("INVALID_ARGUMENT", "Project name is empty", "请输入项目名称。");
    }
    project.doc.name = name.slice(0, 120);
    project.doc.updatedAt = nowIso();
    return this.summaryOf(project);
  }

  async renameCollection(req: { collectionId: string; name: string }): Promise<void> {
    await delay(40);
    const name = req.name.trim();
    if (!name) {
      throw apiError("INVALID_ARGUMENT", "Collection name is empty", "请输入照片集名称。");
    }
    let updated = 0;
    for (const project of this.projects.values()) {
      if (project.doc.collectionId !== req.collectionId) continue;
      project.doc.collectionName = name.slice(0, 120);
      project.doc.updatedAt = nowIso();
      updated += 1;
    }
    if (updated === 0) {
      throw apiError("INVALID_ARGUMENT", "Collection not found", "这个照片集已经不存在了。");
    }
  }

  async syncColorAdjustments(
    req: SyncColorAdjustmentsRequest,
  ): Promise<SyncColorAdjustmentsResult> {
    await delay(60);
    const source = this.requireProject(req.sourceProjectId);
    const templates = source.doc.layers.filter(
      (layer) => layer.kind === "color" && layer.color,
    );
    if (templates.length === 0) {
      throw apiError(
        "INVALID_ARGUMENT",
        "Source has no colour adjustment",
        "当前照片还没有调色调整，先调好一张再同步。",
      );
    }
    let synced = 0;
    const failed: string[] = [];
    const seen = new Set<string>();
    for (const projectId of req.projectIds) {
      if (projectId === req.sourceProjectId || seen.has(projectId)) continue;
      seen.add(projectId);
      const target = this.projects.get(projectId);
      if (!target) {
        failed.push(projectId);
        continue;
      }
      if (target.doc.collectionId !== source.doc.collectionId) {
        failed.push(target.doc.name);
        continue;
      }
      const firstColor = target.doc.layers.findIndex((layer) => layer.kind === "color");
      const insertAt =
        firstColor >= 0 ? firstColor : Math.min(1, target.doc.layers.length);
      const layers = target.doc.layers.filter((layer) => layer.kind !== "color");
      const copies = templates.map((layer) => ({
        ...layer,
        id: uid("layer"),
        createdAt: nowIso(),
        locked: false,
        color: cloneColor(layer.color),
        mask: cloneMask(layer.mask),
      }));
      layers.splice(insertAt, 0, ...copies);
      target.doc.layers = layers;
      this.pushHistory(target, "同步调色调整");
      synced += 1;
    }
    return { synced, failed };
  }

  async deleteProject(req: ProjectRef): Promise<void> {
    await delay(60);
    const project = this.projects.get(req.projectId);
    if (!project) {
      throw apiError("PROJECT_NOT_FOUND", "Project not found", "这个项目已经不存在了。");
    }
    for (const job of this.jobs.filter((item) => item.projectId === req.projectId)) {
      const timer = this.jobTimers.get(job.id);
      if (timer !== undefined) window.clearInterval(timer);
      this.jobTimers.delete(job.id);
    }
    this.jobs = this.jobs.filter((item) => item.projectId !== req.projectId);
    for (const key of [
      project.sourceUrl,
      ...project.doc.layers.map((layer) => layer.previewPath),
    ]) {
      if (key) this.imageCache.delete(key);
    }
    for (const layer of project.doc.layers) {
      if (layer.color?.lutAssetId) this.luts.delete(layer.color.lutAssetId);
    }
    this.projects.delete(req.projectId);
  }

  async clearRenderCache(): Promise<CacheClearResult> {
    return { deletedFiles: 0, freedBytes: 0 };
  }

  async importPhoto(req: ImportPhotoRequest): Promise<DocumentState> {
    if (req.path.startsWith("data:") || /^https?:/i.test(req.path)) {
      const response = await fetch(req.path);
      if (!response.ok) throw apiError("IMPORT_FAILED", "Fetch failed", "无法读取这张照片，请换一张再试。");
      const blob = await response.blob();
      const dataUrl = await readAsDataUrl(blob);
      const fileName = req.path.startsWith("data:") ? "imported-photo.jpg" : req.path.split("/").pop() ?? "imported-photo.jpg";
      return this.createFromDataUrl(dataUrl, fileName, req.path, req.collectionId, req.collectionName);
    }
    throw apiError(
      "DEMO_PATH_UNAVAILABLE",
      "Cannot read local path in browser",
      "浏览器演示模式无法直接读取本地路径，请使用“打开照片”按钮选择文件。",
    );
  }

  async importPhotoBytes(req: ImportPhotoBytesRequest): Promise<DocumentState> {
    const fileName = req.fileName || "photo.jpg";
    if (!/\.(jpe?g|png|tiff?|webp)$/i.test(fileName)) {
      throw apiError("UNSUPPORTED_FORMAT", "Unsupported format", "支持 JPG、PNG、TIFF 和 WebP 照片。");
    }
    const rawBase64 = req.dataBase64.includes(",")
      ? req.dataBase64.slice(req.dataBase64.indexOf(",") + 1)
      : req.dataBase64;
    const estimatedBytes = Math.floor((rawBase64.length * 3) / 4);
    if (estimatedBytes > MAX_IMPORT_BYTES) {
      throw apiError("FILE_TOO_LARGE", "File exceeds 32MB", "照片超过 32 MB，请先压缩或换一张照片。");
    }
    let dataUrl: string;
    try {
      const blob = base64ToBlob(rawBase64, mimeForFileName(fileName));
      dataUrl = await readAsDataUrl(blob);
    } catch {
      throw apiError("INVALID_IMAGE", "Base64 decode failed", "这张照片无法读取，请换一张再试。");
    }
    return this.createFromDataUrl(
      dataUrl,
      fileName,
      `browser://${fileName}`,
      req.collectionId,
      req.collectionName,
    );
  }

  async importLut(req: ImportLutRequest): Promise<LutAssetResult> {
    await delay(20);
    this.requireProject(req.projectId);
    const fileName = req.fileName || "lookup.cube";
    if (!/\.cube$/i.test(fileName)) {
      throw apiError("LUT_INVALID", "not a .cube file", "请选择 .cube 格式的 LUT 文件。");
    }
    const rawBase64 = req.dataBase64 ?? "";
    const encoded = rawBase64.includes(",")
      ? rawBase64.slice(rawBase64.indexOf(",") + 1)
      : rawBase64;
    if (!encoded) {
      throw apiError("LUT_INVALID", "empty lut payload", "LUT 文件内容为空，请重新选择。");
    }
    if (Math.floor((encoded.length * 3) / 4) > MAX_LUT_BYTES) {
      throw apiError("LUT_TOO_LARGE", "cube exceeds 8MB", "LUT 文件过大（最大 8MB）。");
    }
    const blob = base64ToBlob(encoded, "text/plain");
    let cube: CubeLut;
    try {
      cube = parseCube(await blob.text());
    } catch (error) {
      throw apiError(
        "LUT_INVALID",
        error instanceof Error ? error.message : "cube parse failed",
        "这个 .cube 文件无法解析，请确认它是有效的 1D/3D LUT。",
      );
    }
    const assetId = uid("lut");
    this.remember(this.luts, assetId, cube, DemoBackend.MAX_CACHED_LUTS);
    return {
      assetId,
      displayName: fileName,
      dimension: cube.dimension,
      size: cube.size,
      fileBytes: blob.size,
    };
  }

  async renderPreview(req: RenderPreviewRequest): Promise<PreviewResult> {
    const project = this.requireProject(req.projectId);
    if (!project.doc.source) {
      throw apiError("NO_SOURCE", "No source image", "这个项目还没有照片。");
    }
    const started = performance.now();
    await delay(14);
    const requested = req.maxDimension ?? PREVIEW_MAX_DIMENSION;
    const maxDimension = Math.min(PREVIEW_MAX_DIMENSION, Math.max(320, requested));
    const canvas = await this.renderToCanvas(project, maxDimension, req.ignoreCrop === true);
    const path = canvas.toDataURL("image/png");
    const pixels = context2d(canvas).getImageData(0, 0, canvas.width, canvas.height).data;
    return {
      path,
      width: canvas.width,
      height: canvas.height,
      revision: project.doc.revision,
      durationMs: Math.round(performance.now() - started),
      cached: false,
      backend: "cpu",
      histogram: histogramFromRgba(pixels),
    };
  }

  async autoTone(req: ProjectRef): Promise<AutoToneResult> {
    await delay(10);
    const project = this.requireProject(req.projectId);
    const source = project.doc.source;
    if (!source) {
      throw apiError("NO_SOURCE", "No source image", "这个项目还没有照片。");
    }
    const image = await this.loadCachedImage(project.sourceUrl);
    const naturalWidth = image.naturalWidth || source.width;
    const naturalHeight = image.naturalHeight || source.height;
    const scale = Math.min(1, 1024 / Math.max(1, naturalWidth, naturalHeight));
    const width = Math.max(1, Math.round(naturalWidth * scale));
    const height = Math.max(1, Math.round(naturalHeight * scale));
    const canvas = createCanvas(width, height);
    context2d(canvas).drawImage(image, 0, 0, width, height);
    const pixels = context2d(canvas).getImageData(0, 0, width, height).data;
    const suggestion = autoToneFromLumaHistogram(lumaHistogramFromRgba(pixels));
    if (!suggestion) {
      throw apiError(
        "INVALID_ARGUMENT",
        "Source has no analyzable pixels",
        "这张照片没有可分析的像素。",
      );
    }
    return suggestion;
  }

  async updateLayerParams(req: UpdateLayerParamsRequest): Promise<DocumentState> {
    const project = this.requireProject(req.projectId);
    const layer = this.requireEditableLayer(project, req.layerId);
    if (req.color) layer.color = { ...req.color };
    if (req.portrait) layer.portrait = { ...req.portrait };
    if (req.blur) layer.blur = { ...req.blur };
    if (req.retouch)
      layer.retouch = {
        spots: req.retouch.spots.map((spot) => ({ ...spot })),
        warp: req.retouch.warp.map((stroke) => ({ ...stroke, points: stroke.points.map((point) => ({ ...point })) })),
        freeze: req.retouch.freeze.map((stroke) => ({ ...stroke, points: stroke.points.map((point) => ({ ...point })) })),
        freezeGrid: [...req.retouch.freezeGrid],
      };
    this.touch(project);
    return cloneDocument(project.doc);
  }

  async updateLayerParamsMeta(req: UpdateLayerParamsRequest): Promise<DocumentState> {
    const state = await this.updateLayerParams(req);
    // Mirror the desktop contract: envelope only, no per-layer payload.
    return { ...state, layers: [] };
  }

  async checkSegmentationCapability(): Promise<SegmentationCapability> {
    return {
      available: false,
      reason: "浏览器演示不支持本地 ONNX 推理，请在桌面版使用。",
    };
  }

  async runSegmentationMask(): Promise<DocumentState> {
    throw apiError(
      "SEGMENTATION_MODEL_INVALID",
      "segmentation needs the desktop app",
      "浏览器演示不支持本地分割模型，请在桌面版使用。",
    );
  }

  async clearRasterMask(req: ClearRasterMaskRequest): Promise<DocumentState> {
    await delay(12);
    const project = this.requireProject(req.projectId);
    const layer = project.doc.layers.find((item) => item.id === req.layerId);
    if (layer?.mask?.raster) {
      layer.mask = { ...layer.mask, raster: null };
      this.touch(project);
    }
    return cloneDocument(project.doc);
  }

  async listVersions(req: ProjectRef): Promise<VersionSummary[]> {
    const project = this.requireProject(req.projectId);
    return project.versions.map((item) => ({ ...item.summary }));
  }

  async createVersion(req: CreateVersionRequest): Promise<VersionSummary[]> {
    await delay(18);
    const project = this.requireProject(req.projectId);
    const name = req.name.trim();
    if (!name) {
      throw apiError("INVALID_ARGUMENT", "Version name is empty", "请先输入版本名称。");
    }
    if (name.length > 60) {
      throw apiError("INVALID_ARGUMENT", "Version name is too long", "版本名称最多 60 个字符。");
    }
    project.versions.unshift({
      summary: {
        id: uid("version"),
        name,
        revision: project.doc.revision,
        createdAt: nowIso(),
        thumbnailPath: project.doc.source?.previewPath ?? null,
      },
      snapshot: cloneDocument(project.doc),
    });
    return this.listVersions({ projectId: req.projectId });
  }

  async restoreVersion(req: VersionIdRequest): Promise<DocumentState> {
    await delay(24);
    const project = this.requireProject(req.projectId);
    const entry = project.versions.find((item) => item.summary.id === req.versionId);
    if (!entry) {
      throw apiError(
        "VERSION_NOT_FOUND",
        "Version not found",
        "这个版本已经不存在，请刷新列表。",
      );
    }
    // Same shape as the desktop: the previous state becomes one undo step.
    this.pushHistory(project, `恢复版本：${entry.summary.name}`);
    project.doc.layers = cloneLayers(entry.snapshot.layers);
    project.doc.crop = { ...entry.snapshot.crop };
    project.doc.revision = Math.max(project.doc.revision, entry.snapshot.revision) + 1;
    project.doc.updatedAt = nowIso();
    project.lastCommitted = {
      layers: cloneLayers(project.doc.layers),
      crop: { ...project.doc.crop },
    };
    return cloneDocument(project.doc);
  }

  async deleteVersion(req: VersionIdRequest): Promise<VersionSummary[]> {
    await delay(12);
    const project = this.requireProject(req.projectId);
    project.versions = project.versions.filter((item) => item.summary.id !== req.versionId);
    return this.listVersions({ projectId: req.projectId });
  }

  async renderVersionPreview(req: VersionPreviewRequest): Promise<PreviewResult> {
    const project = this.requireProject(req.projectId);
    const entry = project.versions.find((item) => item.summary.id === req.versionId);
    if (!entry) {
      throw apiError(
        "VERSION_NOT_FOUND",
        "Version not found",
        "这个版本已经不存在，请刷新列表。",
      );
    }
    const started = performance.now();
    await delay(10);
    const requested = req.maxDimension ?? PREVIEW_MAX_DIMENSION;
    const maxDimension = Math.min(PREVIEW_MAX_DIMENSION, Math.max(320, requested));
    // Render the snapshot without swapping the live document: renderToCanvas
    // only reads `doc` and `sourceUrl`.
    const canvas = await this.renderToCanvas({ ...project, doc: entry.snapshot }, maxDimension);
    const pixels = context2d(canvas).getImageData(0, 0, canvas.width, canvas.height).data;
    return {
      path: canvas.toDataURL("image/png"),
      width: canvas.width,
      height: canvas.height,
      revision: entry.snapshot.revision,
      durationMs: Math.round(performance.now() - started),
      cached: false,
      backend: "cpu",
      histogram: histogramFromRgba(pixels),
    };
  }

  async commitChanges(req: CommitChangesRequest): Promise<DocumentState> {
    await delay(24);
    const project = this.requireProject(req.projectId);
    this.pushHistory(project, req.label.trim() || "编辑照片");
    return cloneDocument(project.doc);
  }

  async setCrop(req: SetCropRequest): Promise<DocumentState> {
    await delay(24);
    const project = this.requireProject(req.projectId);
    project.doc.crop = clampedCrop(req.crop, project.doc.canvas);
    this.pushHistory(project, "裁剪照片");
    return cloneDocument(project.doc);
  }

  async pickWhiteBalance(req: PickWhiteBalanceRequest): Promise<WhiteBalanceResult> {
    await delay(10);
    const project = this.requireProject(req.projectId);
    const source = project.doc.source;
    if (!source) {
      throw apiError("NO_SOURCE", "No source image", "这个项目还没有照片。");
    }
    if (
      !Number.isFinite(req.x) ||
      !Number.isFinite(req.y) ||
      req.x < 0 ||
      req.x > 1 ||
      req.y < 0 ||
      req.y > 1
    ) {
      throw apiError("INVALID_ARGUMENT", "Invalid sample point", "采样位置无效，请重新点击画面。");
    }

    const image = await this.loadCachedImage(project.sourceUrl);
    const width = image.naturalWidth || source.width;
    const height = image.naturalHeight || source.height;
    const centerX = Math.round(req.x * Math.max(0, width - 1));
    const centerY = Math.round(req.y * Math.max(0, height - 1));
    const radius = Math.min(8, Math.max(2, Math.round(Math.min(width, height) / 500)));
    const left = Math.max(0, centerX - radius);
    const top = Math.max(0, centerY - radius);
    const sampleWidth = Math.min(width, centerX + radius + 1) - left;
    const sampleHeight = Math.min(height, centerY + radius + 1) - top;

    const canvas = createCanvas(Math.max(1, sampleWidth), Math.max(1, sampleHeight));
    const ctx = context2d(canvas);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(image, left, top, sampleWidth, sampleHeight, 0, 0, sampleWidth, sampleHeight);
    const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height).data;

    const sum = [0, 0, 0];
    let count = 0;
    for (let index = 0; index < pixels.length; index += 4) {
      const pixel = [pixels[index], pixels[index + 1], pixels[index + 2]];
      if (pixel.some((value) => value < 6 || value > 249)) continue;
      sum[0] += pixel[0];
      sum[1] += pixel[1];
      sum[2] += pixel[2];
      count += 1;
    }
    if (count === 0) {
      throw apiError(
        "INVALID_ARGUMENT",
        "Sample is clipped",
        "这里太暗或过曝了，请换一块中性灰（例如灰卡、白墙）再试。",
      );
    }

    const average = sum.map((value) => Math.round(value / count)) as [number, number, number];
    const suggestion = neutralWhiteBalance(
      average.map((value) => value / 255) as [number, number, number],
      "display",
    );
    if (!suggestion) {
      throw apiError(
        "INVALID_ARGUMENT",
        "Sample is unusable",
        "这个点不适合作为白平衡参考，请选择中性灰或白色区域。",
      );
    }
    return { ...suggestion, sample: average };
  }

  async setLayerMask(req: SetLayerMaskRequest): Promise<DocumentState> {
    await delay(24);
    const project = this.requireProject(req.projectId);
    const layer = this.requireEditableLayer(project, req.layerId);
    if (
      layer.kind !== "color" &&
      layer.kind !== "portrait" &&
      layer.kind !== "blur"
    ) {
      throw apiError(
        "INVALID_ARGUMENT",
        "Layer does not support masks",
        "只有调色、人像或背景虚化图层支持局部画笔。",
      );
    }
    layer.mask = cloneMask(req.mask);
    const mask = req.mask;
    const hasContent = Boolean(
      mask &&
        (mask.strokes.length > 0 ||
          mask.autoSkin ||
          mask.gradient ||
          mask.raster),
    );
    let label = "清除修饰范围";
    if (mask && hasContent) {
      if (mask.autoSkin && mask.strokes.length === 0 && !mask.gradient) {
        label = "自动肤色范围";
      } else if (mask.gradient && mask.strokes.length === 0 && !mask.autoSkin) {
        label = "渐变修饰范围";
      } else if (mask.gradient) {
        label = "渐变与画笔修饰";
      } else {
        label = "画笔修饰";
      }
    }
    this.pushHistory(project, label);
    return cloneDocument(project.doc);
  }

  async addLayer(req: AddLayerRequest): Promise<DocumentState> {
    await delay(32);
    const project = this.requireProject(req.projectId);
    const layer: LayerState = {
      id: uid("layer"),
      name:
        req.name ??
        (req.kind === "portrait"
          ? "人像精修"
          : req.kind === "retouch"
            ? "局部修复"
            : req.kind === "blur"
              ? "背景虚化"
              : "调色调整"),
      kind: req.kind,
      visible: true,
      locked: false,
      opacity: 1,
      blendMode: "normal",
      color: req.kind === "color" ? { ...DEFAULT_COLOR } : null,
      portrait:
        req.kind === "portrait"
          ? { ...DEFAULT_PORTRAIT, ...req.portrait }
          : null,
      retouch:
        req.kind === "retouch"
          ? { spots: [], warp: [], freeze: [], freezeGrid: [] }
          : null,
      blur: req.kind === "blur" ? { ...DEFAULT_BLUR } : null,
      mask: req.mask ?? null,
      assetId: null,
      previewPath: null,
      createdAt: nowIso(),
    };
    project.doc.layers.push(layer);
    this.pushHistory(
      project,
      req.kind === "portrait"
        ? "添加人像调整"
        : req.kind === "retouch"
          ? "添加修复图层"
          : "添加调色调整",
    );
    return cloneDocument(project.doc);
  }

  async updateLayer(req: UpdateLayerRequest): Promise<DocumentState> {
    const project = this.requireProject(req.projectId);
    const layer = this.requireEditableLayer(project, req.layerId);
    const patch = req.patch;
    const onlyOpacity =
      patch.opacity !== undefined &&
      patch.name === undefined &&
      patch.visible === undefined &&
      patch.locked === undefined &&
      patch.blendMode === undefined;

    if (patch.name !== undefined) layer.name = patch.name;
    if (patch.visible !== undefined) layer.visible = patch.visible;
    if (patch.locked !== undefined) layer.locked = patch.locked;
    if (patch.opacity !== undefined) layer.opacity = clamp(patch.opacity, 0, 1);
    if (patch.blendMode !== undefined) layer.blendMode = patch.blendMode;

    if (onlyOpacity) {
      this.touch(project);
    } else {
      let label = "调整图层";
      if (patch.visible !== undefined) label = patch.visible ? "显示图层" : "隐藏图层";
      else if (patch.blendMode !== undefined) label = "更改混合方式";
      else if (patch.name !== undefined) label = "重命名图层";
      else if (patch.locked !== undefined) label = patch.locked ? "锁定图层" : "解锁图层";
      this.pushHistory(project, label);
    }
    return cloneDocument(project.doc);
  }

  async duplicateLayer(req: DuplicateLayerRequest): Promise<DocumentState> {
    await delay(20);
    const project = this.requireProject(req.projectId);
    const index = project.doc.layers.findIndex((item) => item.id === req.layerId);
    const layer = project.doc.layers[index];
    if (!layer) {
      throw apiError("LAYER_NOT_FOUND", "Layer not found", "这个图层已经不存在了。");
    }
    if (layer.kind === "base" || index === 0) {
      throw apiError(
        "BASE_LAYER_PROTECTED",
        "Base layer is protected",
        "原始照片图层不能复制。",
      );
    }
    const copy = cloneLayers([layer])[0];
    copy.id = uid("layer");
    copy.name = `${layer.name} 副本`;
    copy.locked = false;
    copy.createdAt = nowIso();
    project.doc.layers.splice(index + 1, 0, copy);
    this.pushHistory(project, "复制图层");
    return cloneDocument(project.doc);
  }

  async deleteLayer(req: DeleteLayerRequest): Promise<DocumentState> {
    const project = this.requireProject(req.projectId);
    const layer = project.doc.layers.find((item) => item.id === req.layerId);
    if (!layer) throw apiError("LAYER_NOT_FOUND", "Layer not found", "这个图层已经不存在了。");
    if (layer.kind === "base") {
      throw apiError("BASE_LAYER_PROTECTED", "Base layer is protected", "原始照片图层不能删除。");
    }
    project.doc.layers = project.doc.layers.filter((item) => item.id !== req.layerId);
    this.pushHistory(project, "删除图层");
    return cloneDocument(project.doc);
  }

  async moveLayer(req: MoveLayerRequest): Promise<DocumentState> {
    const project = this.requireProject(req.projectId);
    const editable = project.doc.layers.filter((layer) => layer.kind !== "base");
    const index = editable.findIndex((layer) => layer.id === req.layerId);
    if (index < 0) throw apiError("LAYER_NOT_FOUND", "Layer not found", "这个图层已经不存在了。");
    const target = req.direction === "up" ? index + 1 : index - 1;
    if (target < 0 || target >= editable.length) return cloneDocument(project.doc);
    const temp = editable[index];
    editable[index] = editable[target];
    editable[target] = temp;
    const base = project.doc.layers.find((layer) => layer.kind === "base");
    project.doc.layers = base ? [base, ...editable] : editable;
    this.pushHistory(project, "调整图层顺序");
    return cloneDocument(project.doc);
  }

  async undo(req: ProjectRef): Promise<DocumentState> {
    await delay(20);
    const project = this.requireProject(req.projectId);
    const entry = project.undoStack.pop();
    if (!entry) return cloneDocument(project.doc);
    project.redoStack.push({
      label: entry.label,
      layers: cloneLayers(project.doc.layers),
      crop: { ...project.doc.crop },
    });
    project.doc.layers = cloneLayers(entry.layers);
    project.doc.crop = { ...entry.crop };
    project.lastCommitted = { layers: cloneLayers(entry.layers), crop: { ...entry.crop } };
    this.touch(project);
    return cloneDocument(project.doc);
  }

  async redo(req: ProjectRef): Promise<DocumentState> {
    await delay(20);
    const project = this.requireProject(req.projectId);
    const entry = project.redoStack.pop();
    if (!entry) return cloneDocument(project.doc);
    project.undoStack.push({
      label: entry.label,
      layers: cloneLayers(project.doc.layers),
      crop: { ...project.doc.crop },
    });
    project.doc.layers = cloneLayers(entry.layers);
    project.doc.crop = { ...entry.crop };
    project.lastCommitted = { layers: cloneLayers(entry.layers), crop: { ...entry.crop } };
    this.touch(project);
    return cloneDocument(project.doc);
  }

  async exportImage(req: ExportImageRequest): Promise<ExportResult> {
    const project = this.requireProject(req.projectId);
    if (req.options.format === "tiff") {
      throw apiError("TIFF_DEMO_UNAVAILABLE", "TIFF export requires desktop", "TIFF 导出仅在桌面版可用，浏览器演示可导出 JPG 或 PNG。");
    }
    const started = performance.now();
    const jobId = req.jobId;
    try {
      this.emitExport(jobId, "preparing", 5, "准备导出…");
      await delay(900);
      this.throwIfExportCancelled(jobId);
      const crop = clampedCrop(project.doc.crop, project.doc.canvas);
      const cropLongest = Math.max(
        crop.width || project.doc.canvas.width,
        crop.height || project.doc.canvas.height,
      );
      const target =
        req.options.maxDimension === null
          ? cropLongest
          : Math.min(cropLongest, req.options.maxDimension);
      this.emitExport(jobId, "rendering", 35, "正在渲染全分辨率画面…");
      let canvas = await this.renderToCanvas(project, target);
      this.throwIfExportCancelled(jobId);
      const hasWidth = req.options.width !== null;
      const hasHeight = req.options.height !== null;
      if (hasWidth !== hasHeight) {
        throw apiError("INVALID_ARGUMENT", "Incomplete output size", "自定义导出尺寸无效。");
      }
      if (hasWidth && hasHeight) {
        const width = Number(req.options.width);
        const height = Number(req.options.height);
        if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1) {
          throw apiError("INVALID_ARGUMENT", "Invalid output size", "自定义导出尺寸无效。");
        }
        canvas = coverCanvas(canvas, width, height);
      }
      this.emitExport(jobId, "encoding", 78, "正在编码文件…");
      const watermark = req.options.watermark;
      if (watermark?.imageBase64) {
        let stamp: HTMLImageElement;
        try {
          stamp = await loadImage(watermark.imageBase64);
        } catch {
          throw apiError(
            "INVALID_ARGUMENT",
            "Watermark decode failed",
            "水印图片无法读取，请重新选择。",
          );
        }
        const short = Math.min(canvas.width, canvas.height);
        const targetWidth = Math.max(1, Math.round(short * clamp(watermark.scale, 0.01, 1)));
        const targetHeight = Math.max(
          1,
          Math.round((targetWidth * stamp.height) / Math.max(1, stamp.width)),
        );
        const margin = Math.round(short * clamp(watermark.margin ?? 0.04, 0, 0.5));
        const x = watermark.position.endsWith("Right")
          ? canvas.width - targetWidth - margin
          : watermark.position === "center"
            ? (canvas.width - targetWidth) / 2
            : margin;
        const y = watermark.position.startsWith("bottom")
          ? canvas.height - targetHeight - margin
          : watermark.position === "center"
            ? (canvas.height - targetHeight) / 2
            : margin;
        const ctx = context2d(canvas);
        ctx.save();
        ctx.globalAlpha = clamp(watermark.opacity ?? 1, 0, 1);
        ctx.drawImage(stamp, Math.max(0, x), Math.max(0, y), targetWidth, targetHeight);
        ctx.restore();
      }
      await delay(500);
      this.throwIfExportCancelled(jobId);
      if (req.options.format === "jpeg") {
        const ctx = context2d(canvas);
        ctx.globalCompositeOperation = "destination-over";
        ctx.fillStyle = "#0b0e13";
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        ctx.globalCompositeOperation = "source-over";
      }
      const mime = req.options.format === "jpeg" ? "image/jpeg" : "image/png";
      const dataUrl = canvas.toDataURL(mime, clamp(req.options.quality, 1, 100) / 100);
      this.throwIfExportCancelled(jobId);
      const fileName = req.options.path.split(/[\\/]/).pop() || `luma-export.${req.options.format === "jpeg" ? "jpg" : "png"}`;
      downloadDataUrl(dataUrl, fileName);
      this.emitExport(jobId, "completed", 100, "导出完成。");
      return {
        path: req.options.path,
        bytes: dataUrlBytes(dataUrl),
        width: canvas.width,
        height: canvas.height,
        durationMs: Math.round(performance.now() - started),
        backend: "cpu",
        colorSpace: "sRGB",
        // A canvas data URL cannot carry EXIF or 16-bit samples.
        bitDepth: 8,
        metadataWritten: false,
        keepsLocation: false,
      };
    } finally {
      if (jobId) this.cancelledExports.delete(jobId);
    }
  }

  async cancelExport(req: CancelExportRequest): Promise<boolean> {
    this.cancelledExports.add(req.jobId);
    return true;
  }

  async subscribeExportProgress(
    listener: (progress: ExportProgress) => void,
  ): Promise<() => void> {
    this.exportListeners.add(listener);
    return () => {
      this.exportListeners.delete(listener);
    };
  }

  async analyzePortrait(_req: ProjectRef): Promise<PortraitAnalysisResult> {
    await delay(120);
    return {
      available: false,
      faces: [],
      warnings: [
        {
          code: "FACE_MODEL_UNAVAILABLE",
          message: "当前版本未接入人像识别模型，仅通过肤色范围进行轻度处理。",
        },
      ],
    };
  }

  async analyzePortraitRegions(_req: ProjectRef): Promise<PortraitRegionsResult> {
    await delay(80);
    return {
      available: false,
      faces: [],
      warnings: [
        {
          code: "REGION_MODEL_UNAVAILABLE",
          message: "浏览器演示模式没有五官定位，区域蒙版仅在桌面版可用。",
        },
      ],
    };
  }

  async runAiDenoise(_req: AiDenoiseRequest): Promise<DocumentState> {
    throw apiError(
      "AI_DENOISE_UNAVAILABLE",
      "local ONNX inference requires the desktop runtime",
      "AI 降噪使用本地 ONNX 推理，仅在桌面版可用。",
    );
  }

  async runPhotoHqDenoise(_req: PhotoHqDenoiseRequest): Promise<DocumentState> {
    throw apiError(
      "AI_DENOISE_UNAVAILABLE",
      "Photo-HQ inference requires the desktop runtime",
      "Photo-HQ 人像降噪使用本地 GPU 引擎，仅在桌面版可用。",
    );
  }

  async cancelAiDenoise(_req: JobIdRequest): Promise<boolean> {
    return false;
  }

  async checkAiDenoiseCapability(
    _req: AiCapabilityRequest,
  ): Promise<AiDenoiseCapability> {
    return {
      photoHq: {
        available: false,
        reason: "浏览器演示模式没有本地 GPU 推理",
      },
      onnx: {
        available: false,
        reason: "浏览器演示模式没有本地 ONNX 推理",
      },
    };
  }

  async listProviders(): Promise<ProviderInfo[]> {
    return this.providers.map((provider) => ({ ...provider, capabilities: { ...provider.capabilities } }));
  }

  async saveProviderConfig(req: ProviderConfigInput): Promise<ProviderInfo> {
    await delay(120);
    const existing = req.id ? this.providers.find((provider) => provider.id === req.id) : undefined;
    const name = req.name.trim() || (req.kind === "mock" ? "本地演示服务" : "自定义服务");
    const capabilities: ProviderCapabilities =
      req.kind === "mock"
        ? {
            supportsMask: false,
            supportsReferenceImages: false,
            supportsMultipleImages: false,
            returnsImage: true,
            supportsStructuredParameters: false,
            supportsCancellation: true,
            maxWidth: 1400,
            maxHeight: 1400,
            maxInputBytes: null,
          }
        : {
            supportsMask: false,
            supportsReferenceImages: false,
            supportsMultipleImages: false,
            returnsImage: true,
            supportsStructuredParameters: false,
            supportsCancellation: true,
            maxWidth: 2048,
            maxHeight: 2048,
            maxInputBytes: 20 * 1024 * 1024,
          };
    if (existing) {
      existing.name = name;
      existing.kind = req.kind;
      existing.baseUrl = req.baseUrl?.trim() || null;
      existing.model = req.model?.trim() || null;
      existing.capabilities = capabilities;
      return { ...existing, capabilities: { ...capabilities } };
    }
    const provider: ProviderInfo = {
      id: uid("provider"),
      name,
      kind: req.kind,
      baseUrl: req.baseUrl?.trim() || null,
      model: req.model?.trim() || null,
      hasCredential: false,
      capabilities,
      createdAt: nowIso(),
    };
    this.providers = [...this.providers, provider];
    return { ...provider, capabilities: { ...capabilities } };
  }

  async setProviderCredential(req: SetProviderCredentialRequest): Promise<ProviderInfo> {
    await delay(80);
    const provider = this.providers.find((item) => item.id === req.providerId);
    if (!provider) throw apiError("PROVIDER_NOT_FOUND", "Provider not found", "找不到这个 AI 服务，请刷新后再试。");
    // The demo intentionally keeps only a boolean; the key is never stored.
    provider.hasCredential = req.apiKey.trim().length > 0;
    return { ...provider, capabilities: { ...provider.capabilities } };
  }

  async listProviderModels(req: ProviderIdRequest): Promise<string[]> {
    await delay(180);
    const provider = this.providers.find((item) => item.id === req.providerId);
    if (!provider) {
      throw apiError("PROVIDER_NOT_FOUND", "Provider not found", "找不到这个 AI 服务。");
    }
    if (provider.kind === "mock") {
      return ["luma-demo-v1"];
    }
    if (!provider.hasCredential) {
      throw apiError(
        "AUTH_FAILED",
        "Credential missing",
        "尚未保存 API Key，请先保存密钥再获取模型列表。",
      );
    }
    return provider.model ? [provider.model] : [];
  }

  async testProvider(req: ProviderIdRequest): Promise<ProviderTestResult> {
    const started = performance.now();
    await delay(420);
    const provider = this.providers.find((item) => item.id === req.providerId);
    if (!provider) throw apiError("PROVIDER_NOT_FOUND", "Provider not found", "找不到这个 AI 服务。");
    if (provider.kind === "mock") {
      return {
        ok: true,
        latencyMs: Math.round(performance.now() - started),
        message: "本地演示服务运行正常，不会上传图片。",
      };
    }
    if (!provider.hasCredential) {
      return {
        ok: false,
        latencyMs: Math.round(performance.now() - started),
        message: "还没有保存 API Key。浏览器演示不会发起真实网络请求。",
      };
    }
    return {
      ok: true,
      latencyMs: Math.round(performance.now() - started),
      message: "配置已保存。演示环境使用本地模拟结果，真正连接测试将在桌面版执行。",
    };
  }

  async submitAiJob(req: SubmitAiJobRequest): Promise<{ jobId: string }> {
    const project = this.requireProject(req.projectId);
    const provider = this.providers.find((item) => item.id === req.providerId);
    if (!provider) throw apiError("PROVIDER_NOT_FOUND", "Provider not found", "请先选择一个可用的 AI 服务。");
    if (!req.prompt.trim()) {
      throw apiError("EMPTY_PROMPT", "Prompt is empty", "先写一句想要的效果，再开始生成。");
    }
    const jobId = uid("job");
    const timestamp = nowIso();
    const job: AiJobInfo = {
      id: jobId,
      projectId: project.doc.id,
      providerId: provider.id,
      providerName: provider.name,
      operation: req.operation,
      prompt: req.prompt.trim(),
      status: "running",
      progress: 4,
      message: provider.kind === "mock" ? "正在本地生成演示效果…" : "演示环境正在本地模拟结果…",
      resultAssetId: null,
      resultPath: null,
      errorCode: null,
      userMessage: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    };
    this.jobs = [job, ...this.jobs].slice(0, DemoBackend.MAX_JOBS);
    this.emitJob(job);
    let progress = 4;
    const timer = window.setInterval(() => {
      progress = Math.min(94, progress + 12 + Math.random() * 5);
      const current = this.jobs.find((item) => item.id === jobId);
      if (!current || current.status !== "running") {
        window.clearInterval(timer);
        this.jobTimers.delete(jobId);
        return;
      }
      current.progress = Math.round(progress);
      current.updatedAt = nowIso();
      if (progress >= 94) {
        current.message = "正在整理结果…";
        window.clearInterval(timer);
        this.jobTimers.delete(jobId);
        void this.finishDemoJob(jobId);
      }
      this.emitJob(current);
    }, 190);
    this.jobTimers.set(jobId, timer);
    return { jobId };
  }

  async cancelAiJob(req: JobIdRequest): Promise<void> {
    const timer = this.jobTimers.get(req.jobId);
    if (timer !== undefined) {
      window.clearInterval(timer);
      this.jobTimers.delete(req.jobId);
    }
    this.patchJob(req.jobId, {
      status: "cancelled",
      progress: null,
      message: "已取消，这张照片没有被修改。",
      updatedAt: nowIso(),
    });
  }

  async listAiJobs(req: ProjectRef): Promise<AiJobInfo[]> {
    return this.jobs
      .filter((job) => job.projectId === req.projectId)
      .map((job) => ({ ...job }))
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  }

  async applyAiResult(req: ApplyAiResultRequest): Promise<DocumentState> {
    await delay(48);
    const project = this.requireProject(req.projectId);
    const job = this.jobs.find((item) => item.id === req.jobId);
    if (!job || job.status !== "completed" || !job.resultPath) {
      throw apiError("AI_RESULT_NOT_READY", "AI result not ready", "这张 AI 结果还没有准备好，请稍后再试。");
    }
    const resultAssetId = job.resultAssetId ?? job.id;
    if (
      project.doc.layers.some(
        (layer) => layer.kind === "aiResult" && layer.assetId === resultAssetId,
      )
    ) {
      throw apiError("INVALID_ARGUMENT", "AI result already applied", "该 AI 结果已经应用到项目。");
    }
    const shortPrompt = job.prompt.length > 18 ? `${job.prompt.slice(0, 18)}…` : job.prompt;
    const layer: LayerState = {
      id: uid("layer"),
      name: `AI · ${shortPrompt || "新效果"}`,
      kind: "aiResult",
      visible: true,
      locked: false,
      opacity: 0.9,
      blendMode: "normal",
      color: null,
      portrait: null,
      retouch: null,
      blur: null,
      mask: null,
      assetId: resultAssetId,
      previewPath: job.resultPath,
      createdAt: nowIso(),
    };
    project.doc.layers.push(layer);
    this.pushHistory(project, "应用 AI 结果");
    return cloneDocument(project.doc);
  }

  async subscribeAiJobs(listener: (job: AiJobInfo) => void): Promise<() => void> {
    this.jobListeners.add(listener);
    return () => {
      this.jobListeners.delete(listener);
    };
  }

  async pickImage(): Promise<string | null> {
    return null;
  }

  async pickImages(): Promise<string[]> {
    return [];
  }

  async pickLutFile(): Promise<string | null> {
    return null;
  }

  async pickDenoiseModel(): Promise<string | null> {
    return null;
  }

  async pickExportPath(defaultName: string, _format: ExportFormat): Promise<string | null> {
    return defaultName;
  }

  async pickDirectory(): Promise<string | null> {
    return null;
  }

  /* ------------------------------------------------------------------ */
  /* Internals                                                           */
  /* ------------------------------------------------------------------ */

  private async createFromDataUrl(
    dataUrl: string,
    fileName: string,
    originalPath: string,
    collectionId?: string | null,
    collectionName?: string | null,
  ): Promise<DocumentState> {
    let image: HTMLImageElement;
    try {
      image = await loadImage(dataUrl);
    } catch {
      throw apiError("INVALID_IMAGE", "Image decode failed", "这张照片无法读取，请换一张 JPG、PNG 或 WebP 图片。");
    }
    if (image.naturalWidth * image.naturalHeight > 100_000_000) {
      throw apiError("IMAGE_TOO_LARGE", "Image exceeds 100MP", "照片尺寸过大，请先缩小到 1 亿像素以内。");
    }
    const thumbnailCanvas = createCanvas(480, 480);
    const thumbCtx = context2d(thumbnailCanvas);
    const thumbScale = Math.min(480 / image.naturalWidth, 480 / image.naturalHeight, 1);
    thumbnailCanvas.width = Math.max(1, Math.round(image.naturalWidth * thumbScale));
    thumbnailCanvas.height = Math.max(1, Math.round(image.naturalHeight * thumbScale));
    thumbCtx.drawImage(image, 0, 0, thumbnailCanvas.width, thumbnailCanvas.height);
    const thumbnailPath = thumbnailCanvas.toDataURL("image/png");

    const previewScale = Math.min(
      1,
      PREVIEW_MAX_DIMENSION / Math.max(image.naturalWidth, image.naturalHeight),
    );
    const previewCanvas = createCanvas(
      image.naturalWidth * previewScale,
      image.naturalHeight * previewScale,
    );
    context2d(previewCanvas).drawImage(image, 0, 0, previewCanvas.width, previewCanvas.height);
    const sourcePreviewPath = previewCanvas.toDataURL("image/png");

    const id = uid("project");
    const timestamp = nowIso();
    const format = extensionOf(fileName) || "jpeg";
    const source: SourceAssetState = {
      assetId: uid("asset"),
      displayName: fileName,
      width: image.naturalWidth,
      height: image.naturalHeight,
      format: format === "jpg" ? "jpeg" : format,
      originalPath,
      previewPath: sourcePreviewPath,
      thumbnailPath,
      // The browser colour-manages decoding itself, so the demo always works
      // from an sRGB canvas and has no source profile to preserve.
      colorSpace: "sRGB",
      hasSourceProfile: false,
    };
    const baseLayer: LayerState = {
      id: uid("base"),
      name: "原始照片",
      kind: "base",
      visible: true,
      locked: true,
      opacity: 1,
      blendMode: "normal",
      color: null,
      portrait: null,
      retouch: null,
      blur: null,
      mask: null,
      assetId: source.assetId,
      previewPath: thumbnailPath,
      createdAt: timestamp,
    };
    const existingCollection = collectionId
      ? [...this.projects.values()].find(
          (project) => project.doc.collectionId === collectionId,
        )
      : undefined;
    const resolvedCollectionId = collectionId || uid("collection");
    const doc: DocumentState = {
      id,
      name: displayNameOf(fileName),
      collectionId: resolvedCollectionId,
      collectionName:
        existingCollection?.doc.collectionName ??
        (collectionName?.trim() || displayNameOf(fileName)),
      schemaVersion: 2,
      revision: 1,
      createdAt: timestamp,
      updatedAt: timestamp,
      canvas: { width: source.width, height: source.height, workingColorSpace: "linear-srgb" },
      crop: { x: 0, y: 0, width: source.width, height: source.height },
      source,
      layers: [baseLayer],
      canUndo: false,
      canRedo: false,
      history: [],
    };
    this.projects.set(id, {
      doc,
      sourceUrl: dataUrl,
      undoStack: [],
      redoStack: [],
      lastCommitted: { layers: cloneLayers(doc.layers), crop: { ...doc.crop } },
      versions: [],
    });
    this.remember(
      this.imageCache,
      dataUrl,
      Promise.resolve(image),
      DemoBackend.MAX_CACHED_IMAGES,
    );
    return cloneDocument(doc);
  }

  /**
   * Everything the desktop backend writes to disk is an in-memory object here,
   * so the maps are capped and evicted with their project: a decoded preview is
   * several megabytes and a 64³ cube is ~4.7 MB.
   */
  private static readonly MAX_CACHED_IMAGES = 6;
  private static readonly MAX_CACHED_LUTS = 8;
  private static readonly MAX_JOBS = 50;

  private remember<K, V>(map: Map<K, V>, key: K, value: V, limit: number): void {
    map.set(key, value);
    while (map.size > limit) {
      const oldest = map.keys().next();
      if (oldest.done) break;
      map.delete(oldest.value);
    }
  }

  private summaryOf(project: DemoProject): ProjectSummary {    return {
      id: project.doc.id,
      name: project.doc.name,
      collectionId: project.doc.collectionId,
      collectionName: project.doc.collectionName,
      width: project.doc.canvas.width,
      height: project.doc.canvas.height,
      updatedAt: project.doc.updatedAt,
      thumbnailPath: project.doc.source?.thumbnailPath ?? null,
    };
  }

  private requireProject(projectId: string): DemoProject {
    const project = this.projects.get(projectId);
    if (!project) {
      throw apiError("PROJECT_NOT_FOUND", "Project not found", "找不到这个项目，请重新打开一张照片。");
    }
    return project;
  }

  private requireEditableLayer(project: DemoProject, layerId: string): LayerState {
    const layer = project.doc.layers.find((item) => item.id === layerId);
    if (!layer) throw apiError("LAYER_NOT_FOUND", "Layer not found", "这个图层已经不存在了。");
    if (layer.kind === "base") {
      throw apiError("BASE_LAYER_PROTECTED", "Base layer is read-only", "原始照片只读，请新建调整图层后再编辑。");
    }
    return layer;
  }

  private touch(project: DemoProject): void {
    project.doc.revision += 1;
    project.doc.updatedAt = nowIso();
    project.doc.canUndo = project.undoStack.length > 0;
    project.doc.canRedo = project.redoStack.length > 0;
    project.doc.history = project.undoStack.slice(-24).map((entry) => entry.label);
  }

  private pushHistory(project: DemoProject, label: string): void {
    const current = {
      layers: cloneLayers(project.doc.layers),
      crop: { ...project.doc.crop },
    };
    if (
      JSON.stringify(current) === JSON.stringify(project.lastCommitted)
    ) {
      this.touch(project);
      return;
    }
    project.undoStack.push({
      label,
      layers: cloneLayers(project.lastCommitted.layers),
      crop: { ...project.lastCommitted.crop },
    });
    if (project.undoStack.length > 60) project.undoStack.shift();
    project.redoStack = [];
    project.lastCommitted = current;
    this.touch(project);
  }

  private async renderToCanvas(
    project: DemoProject,
    maxDimension: number,
    ignoreCrop = false,
  ): Promise<HTMLCanvasElement> {
    const { doc } = project;
    const source = doc.source;
    if (!source) {
      throw apiError("NO_SOURCE", "No source image", "这个项目还没有照片。");
    }
    // Decoded originals live in the bounded LRU, not on the project: keeping an
    // HTMLImageElement per open project made memory grow with every import,
    // while dropping to the preview image would cap export resolution.
    const image = await this.loadCachedImage(project.sourceUrl);
    const sourceWidth = doc.canvas.width;
    const sourceHeight = doc.canvas.height;
    const crop = clampedCrop(doc.crop, doc.canvas);
    const cropWidth = ignoreCrop ? sourceWidth : crop.width || sourceWidth;
    const cropHeight = ignoreCrop ? sourceHeight : crop.height || sourceHeight;
    const scale = Math.min(1, maxDimension / Math.max(cropWidth, cropHeight));
    const width = Math.max(1, Math.round(sourceWidth * scale));
    const height = Math.max(1, Math.round(sourceHeight * scale));
    const canvas = createCanvas(width, height);
    const ctx = context2d(canvas);
    ctx.fillStyle = "#0b0e13";
    ctx.fillRect(0, 0, width, height);

    const base = doc.layers.find((layer) => layer.kind === "base");
    if (!base || base.visible) {
      ctx.drawImage(image, 0, 0, width, height);
    }

    for (const layer of doc.layers) {
      if (layer.kind === "base") continue;
      if (!layer.visible || layer.opacity <= 0) continue;
      if (layer.kind === "color" && layer.color) {
        const mask = layer.mask
          ? rasterizeMask(layer.mask, canvas, sourceWidth, sourceHeight, crop, width, height)
          : null;
        const cube = layer.color.lutAssetId ? (this.luts.get(layer.color.lutAssetId) ?? null) : null;
        applyColorToCanvas(canvas, layer.color, layer.opacity, mask, cube);
      } else if (layer.kind === "portrait" && layer.portrait) {
        const mask = layer.mask
          ? rasterizeMask(layer.mask, canvas, sourceWidth, sourceHeight, crop, width, height)
          : null;
        applyPortraitToCanvas(canvas, layer.portrait, layer.opacity, mask);
      } else if (layer.kind === "blur" && layer.blur) {
        const mask = layer.mask
          ? rasterizeMask(layer.mask, canvas, sourceWidth, sourceHeight, crop, width, height)
          : null;
        applyBlurToCanvas(canvas, layer.blur.radius, layer.opacity, mask);
      } else if (layer.kind === "aiResult" && layer.previewPath) {
        try {
          const resultImage = await this.loadCachedImage(layer.previewPath);
          ctx.save();
          ctx.globalAlpha = clamp01(layer.opacity);
          ctx.globalCompositeOperation = BLEND_TO_COMPOSITE[layer.blendMode] ?? "source-over";
          drawImageCover(ctx, resultImage, width, height);
          ctx.restore();
        } catch {
          // A missing AI result should not break the rest of the render.
        }
      }
    }

    if (
      !ignoreCrop &&
      crop.width > 0 &&
      crop.height > 0 &&
      !(crop.x === 0 && crop.y === 0 && crop.width === sourceWidth && crop.height === sourceHeight)
    ) {
      const scaleX = width / sourceWidth;
      const scaleY = height / sourceHeight;
      const left = Math.min(width - 1, Math.round(crop.x * scaleX));
      const top = Math.min(height - 1, Math.round(crop.y * scaleY));
      const cropWidthScaled = Math.max(1, Math.min(width - left, Math.round(crop.width * scaleX)));
      const cropHeightScaled = Math.max(1, Math.min(height - top, Math.round(crop.height * scaleY)));
      const out = createCanvas(cropWidthScaled, cropHeightScaled);
      const outCtx = context2d(out);
      outCtx.drawImage(
        canvas,
        left,
        top,
        cropWidthScaled,
        cropHeightScaled,
        0,
        0,
        cropWidthScaled,
        cropHeightScaled,
      );
      return out;
    }
    return canvas;
  }

  private loadCachedImage(src: string): Promise<HTMLImageElement> {
    const cached = this.imageCache.get(src);
    if (cached) return cached;
    const promise = loadImage(src);
    this.remember(this.imageCache, src, promise, DemoBackend.MAX_CACHED_IMAGES);
    return promise;
  }

  private emitJob(job: AiJobInfo): void {
    const snapshot = { ...job };
    for (const listener of this.jobListeners) {
      listener(snapshot);
    }
  }

  private patchJob(jobId: string, patch: Partial<AiJobInfo>): void {
    const index = this.jobs.findIndex((job) => job.id === jobId);
    if (index < 0) return;
    const next: AiJobInfo = { ...this.jobs[index], ...patch };
    this.jobs[index] = next;
    this.emitJob(next);
  }

  private async finishDemoJob(jobId: string): Promise<void> {
    const job = this.jobs.find((item) => item.id === jobId);
    if (!job || job.status !== "running") return;
    const project = this.projects.get(job.projectId);
    if (!project) {
      this.patchJob(jobId, {
        status: "failed",
        progress: null,
        errorCode: "PROJECT_NOT_FOUND",
        userMessage: "项目已关闭，AI 结果无法保存。",
        updatedAt: nowIso(),
      });
      return;
    }
    await delay(320);
    const current = this.jobs.find((item) => item.id === jobId);
    if (!current || current.status !== "running") return;
    try {
      const resultPath = await this.generateDemoResult(project);
      this.patchJob(jobId, {
        status: "completed" satisfies AiJobStatus,
        progress: 100,
        message: "生成完成，可作为新图层加入。",
        resultAssetId: jobId,
        resultPath,
        updatedAt: nowIso(),
      });
    } catch {
      this.patchJob(jobId, {
        status: "failed",
        progress: null,
        errorCode: "RENDER_FAILED",
        userMessage: "这次生成没有完成，原图没有被修改，可以再试一次。",
        updatedAt: nowIso(),
      });
    }
  }

  private async generateDemoResult(project: DemoProject): Promise<string> {
    // Always generate from the uncropped source preview so the result aligns
    // with the full-canvas AI layer and the document crop is applied once.
    const sourcePath = project.doc.source?.previewPath;
    if (!sourcePath) throw new Error("no preview");
    const image = await this.loadCachedImage(sourcePath);
    const max = 1200;
    const scale = Math.min(1, max / Math.max(image.naturalWidth, image.naturalHeight));
    const canvas = createCanvas(image.naturalWidth * scale, image.naturalHeight * scale);
    const ctx = context2d(canvas);
    ctx.filter = "saturate(1.07) contrast(1.03) brightness(1.025)";
    ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
    ctx.filter = "none";

    ctx.save();
    ctx.globalCompositeOperation = "soft-light";
    const warm = ctx.createLinearGradient(0, 0, canvas.width, canvas.height);
    warm.addColorStop(0, "rgba(255, 190, 130, 0.42)");
    warm.addColorStop(0.55, "rgba(255, 170, 120, 0.18)");
    warm.addColorStop(1, "rgba(120, 90, 200, 0.18)");
    ctx.fillStyle = warm;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.restore();

    ctx.save();
    const glow = ctx.createRadialGradient(
      canvas.width * 0.34,
      canvas.height * 0.24,
      canvas.width * 0.05,
      canvas.width * 0.34,
      canvas.height * 0.24,
      canvas.width * 0.62,
    );
    glow.addColorStop(0, "rgba(255, 233, 204, 0.2)");
    glow.addColorStop(1, "rgba(255, 233, 204, 0)");
    ctx.globalCompositeOperation = "screen";
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.restore();

    ctx.save();
    const vignette = ctx.createRadialGradient(
      canvas.width / 2,
      canvas.height / 2,
      Math.min(canvas.width, canvas.height) * 0.32,
      canvas.width / 2,
      canvas.height / 2,
      Math.max(canvas.width, canvas.height) * 0.72,
    );
    vignette.addColorStop(0, "rgba(16, 9, 4, 0)");
    vignette.addColorStop(1, "rgba(16, 9, 4, 0.22)");
    ctx.fillStyle = vignette;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.restore();

    return canvas.toDataURL("image/png");
  }
}
