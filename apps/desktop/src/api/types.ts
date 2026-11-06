/**
 * Types mirroring docs/ipc-contract.md (v1) exactly.
 * Rust structs are serialized with camelCase field names.
 */

export interface ApiError {
  code: string;
  message: string;
  userMessage: string;
}

export interface AppInfo {
  version: string;
  backend: "cpu" | "wgpu";
  gpuAvailable: boolean;
  rawAvailable: boolean;
  faceModelAvailable: boolean;
  workspaceDir: string;
  supportedImageFormats: string[];
}

export interface ProjectSummary {
  id: string;
  name: string;
  /** Photo set this photo belongs to. */
  collectionId: string;
  collectionName: string;
  width: number;
  height: number;
  updatedAt: string;
  thumbnailPath: string | null;
}

/** A named snapshot of a project document; survives restarts, unlike undo. */
export interface VersionSummary {
  id: string;
  name: string;
  revision: number;
  createdAt: string;
  thumbnailPath: string | null;
}

export interface CreateVersionRequest {
  projectId: string;
  name: string;
}

export interface VersionIdRequest {
  projectId: string;
  versionId: string;
}

export interface VersionPreviewRequest {
  projectId: string;
  versionId: string;
  maxDimension?: number;
}

export interface CollectionSummary {
  id: string;
  name: string;
  photoCount: number;
  updatedAt: string;
  coverThumbnailPath: string | null;
  latestProjectId: string;
}

export interface SourceAssetState {
  assetId: string;
  displayName: string;
  width: number;
  height: number;
  format: string;
  originalPath: string;
  previewPath: string;
  thumbnailPath: string;
  /** Colour space the file declared, e.g. `sRGB`, `Display P3`. */
  colorSpace: string;
  /** True when import remapped the pixels and kept the original profile. */
  hasSourceProfile: boolean;
}

export interface CanvasState {
  width: number;
  height: number;
  workingColorSpace: "linear-srgb";
}

export interface CropState {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Clockwise straighten angle in degrees; x/y/width/height are in the rotated frame. */
  angle?: number;
}

export type BlendMode = "normal" | "multiply" | "screen" | "overlay" | "softLight";
export type LayerKind = "base" | "portrait" | "color" | "aiResult" | "retouch";

export type RetouchMode = "clone" | "heal";

export interface RetouchSpot {
  mode: RetouchMode;
  /** Normalized source-image coordinates. */
  x: number;
  y: number;
  /** Relative to the shorter source side. */
  radius: number;
  sourceX: number | null;
  sourceY: number | null;
  /** 0 = fully feathered edge, 1 = hard edge. */
  hardness: number;
  opacity: number;
}

export type LiquifyMode = "push" | "pinch" | "bulge";

export interface LiquifyStroke {
  /** Normalized source-image coordinates. */
  points: MaskPoint[];
  /** Relative to the shorter source side. */
  radius: number;
  /** 0..1, scaled by the retouch layer opacity. */
  strength: number;
  mode: LiquifyMode;
}

export interface RetouchParams {
  spots: RetouchSpot[];
  warp: LiquifyStroke[];
  /** Painted protection for the warp pass; coverage 1 means frozen. */
  freeze: MaskStroke[];
  /** Row-major 16x16 grid over the full frame; true keeps a cell still. */
  freezeGrid: boolean[];
}

export interface ColorParams {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  temperature: number;
  tint: number;
  saturation: number;
  vibrance: number;
  clarity: number;
  sharpness: number;
  /** -180..180 degree hue shift. */
  hue: number;
  /** -100..100 HSL saturation multiplier. */
  hslSaturation: number;
  /** -100..100 HSL lightness adjustment. */
  hslLightness: number;
  /** Five fixed input positions, 0..1. */
  curveX: number[];
  /** Five curve outputs, 0..1. */
  curveY: number[];
  levelsInBlack: number;
  levelsInWhite: number;
  levelsGamma: number;
  levelsOutBlack: number;
  levelsOutWhite: number;
  /** 0..100 luminance noise reduction. */
  noiseLuma: number;
  /** 0..100 chroma (colour) noise reduction. */
  noiseChroma: number;
  /** -100..100 vignette; negative brightens the corners. */
  vignette: number;
  /** 0..100 film grain. */
  grain: number;
  /** 0..100 haze removal. */
  dehaze: number;
  /** 0..100 purple/green fringe correction. */
  chromatic: number;
  /** Imported `.cube` LUT asset id, or null when no look transform is used. */
  lutAssetId: string | null;
  /** 0..1 blend between the graded image and the LUT result. */
  lutStrength: number;
}

/** One measured feature ellipse in normalized source coordinates. */
export interface PortraitEllipse {
  centerX: number;
  centerY: number;
  radiusX: number;
  radiusY: number;
  angle: number;
  feather: number;
}

/** Geometry consumed by the fine-repair effects. */
export interface PortraitGeometry {
  face?: PortraitEllipse | null;
  leftEye?: PortraitEllipse | null;
  rightEye?: PortraitEllipse | null;
  leftBrow?: PortraitEllipse | null;
  rightBrow?: PortraitEllipse | null;
  mouth?: PortraitEllipse | null;
  lips?: PortraitEllipse | null;
  teeth?: PortraitEllipse | null;
  neck?: PortraitEllipse | null;
}

export interface PortraitParams {
  intensity: number;
  smooth: number;
  texture: number;
  evenSkin: number;
  brighten: number;
  bodySmooth: number;
  eyeEnhance: number;
  teethWhiten: number;
  blemish: number;
  wrinkle: number;
  darkCircle: number;
  mole: number;
  freckle: number;
  birthmark: number;
  tattoo: number;
  /** Natural skin whitening; does not claim to change real skin tone. */
  whiten: number;
  /** -100 cool .. 100 warm, applied by whitening. */
  whitenWarmth: number;
  /** Warm/rosy tint, strongest on the cheeks when geometry exists. */
  rosy: number;
  /** Under-eye bag shadow correction (not structural reshaping). */
  eyeBag: number;
  wrinkleForehead: number;
  wrinkleGlabella: number;
  wrinkleCrowFeet: number;
  wrinkleNasolabial: number;
  wrinkleMouthCorner: number;
  wrinkleUnderEye: number;
  wrinkleNeck: number;
  /** A body-beauty layer uses the body skin mask and excludes the face. */
  bodyMode: boolean;
  bodyBlemish: number;
  bodyEven: number;
  bodyBrighten: number;
  bodyTexture: number;
  bodyWhiten: number;
  /** Horizontal narrowing inside the layer mask; 0..100, default 0. */
  bodySlim: number;
  /** Vertical stretch of the mask's lower half (legs); 0..100, default 0. */
  bodyLengthen: number;
  /** Local geometric shaping; needs measured face geometry. */
  faceSlim: number;
  eyeSize: number;
  noseSlim: number;
  /** 0 keeps the automatic smoothing radius. */
  smoothRadius: number;
  /** Extra desaturation for a translucent whitening look. */
  whitenSaturation: number;
  oilControl: number;
  eyeWhite: number;
  irisDetail: number;
  teethBrighten: number;
  lipTint: number;
  /** -100 cool/berry .. 100 warm/coral. */
  lipWarmth: number;
  noseContour: number;
  /** Warm eyeshadow on the upper lid. */
  eyeshadow: number;
  /** Darkens and warms the brows. */
  browTint: number;
  jawWidth: number;
  /** -100 shorter .. 100 longer. */
  chinLength: number;
  /** -100 closer .. 100 wider. */
  eyeDistance: number;
  /** Detected face index for per-face portrait layers; absent/null for global. */
  faceIndex?: number | null;
  /** Measured feature geometry for the fine-repair effects. */
  geometry?: PortraitGeometry | null;
}

export interface MaskPoint {
  x: number;
  y: number;
}

export interface MaskStroke {
  points: MaskPoint[];
  /** Radius as a fraction of the source's shortest side. */
  radius: number;
  feather: number;
  erase: boolean;
}

export type MaskGradientKind = "linear" | "radial";

export interface MaskGradient {
  kind: MaskGradientKind;
  /** Opaque end for linear gradients, center for radial gradients. */
  start: MaskPoint;
  /** Transparent end for linear gradients, radius handle for radial gradients. */
  end: MaskPoint;
}

/** One measured facial region inside a mask, in normalized source coordinates. */
export interface MaskRegion {
  kind: string;
  centerX: number;
  centerY: number;
  radiusX: number;
  radiusY: number;
  angle: number;
  feather: number;
  strength: number;
}

export interface LayerMask {
  strokes: MaskStroke[];
  /** Heuristic skin-tone selection; not semantic face segmentation. */
  autoSkin: boolean;
  /** Facial regions produced by the region analyser. */
  regions?: MaskRegion[];
  gradient?: MaskGradient | null;
  inverted?: boolean;
  /** Global edge softening, 0..1. */
  feather?: number;
  /** Overall mask strength, 0..1. */
  density?: number;
  /**
   * Pixel coverage from a segmentation model, stored as a project asset. The
   * renderer merges it with the vector sources by taking the maximum (ADR-014).
   */
  raster?: MaskRaster | null;
}

export interface MaskRaster {
  assetId: string;
  /** Coverage above this value is kept, 0..1. */
  threshold: number;
  /** Softness of the threshold transition, in coverage units. */
  softness: number;
  /** Provider label for diagnostics, e.g. `onnx:foreground`. */
  source: string;
}

export interface SegmentationCapability {
  available: boolean;
  reason: string | null;
}

export interface SegmentationCapabilityRequest {
  modelPath?: string | null;
}

export interface SegmentMaskRequest {
  projectId: string;
  layerId: string;
  modelPath: string;
  threshold?: number;
  softness?: number;
}

export interface ClearRasterMaskRequest {
  projectId: string;
  layerId: string;
}

export interface LayerState {
  id: string;
  name: string;
  kind: LayerKind;
  visible: boolean;
  locked: boolean;
  opacity: number;
  blendMode: BlendMode;
  color: ColorParams | null;
  portrait: PortraitParams | null;
  retouch: RetouchParams | null;
  mask: LayerMask | null;
  assetId: string | null;
  previewPath: string | null;
  createdAt: string;
}

export interface DocumentState {
  id: string;
  name: string;
  collectionId: string;
  collectionName: string;
  schemaVersion: number;
  revision: number;
  createdAt: string;
  updatedAt: string;
  canvas: CanvasState;
  crop: CropState;
  source: SourceAssetState | null;
  layers: LayerState[];
  canUndo: boolean;
  canRedo: boolean;
  history: string[];
}

export interface Histogram {
  red: number[];
  green: number[];
  blue: number[];
  luma: number[];
}

export interface AutoToneResult {
  exposure: number;
  contrast: number;
  whites: number;
  blacks: number;
}

export interface PreviewResult {
  path: string;
  width: number;
  height: number;
  revision: number;
  durationMs: number;
  cached: boolean;
  /** "wgpu" | "cpu" | "cache" */
  backend: string;
  histogram: Histogram | null;
}

export type ExportFormat = "jpeg" | "png" | "tiff" | "webp";

export type ExportColorSpace = "srgb" | "displayP3" | "source";
export type ExportMetadata = "keep" | "privacy" | "none";
export type ExportBitDepth = "eight" | "sixteen";

export type WatermarkPosition =
  | "topLeft"
  | "topRight"
  | "bottomLeft"
  | "bottomRight"
  | "center";

/** Rasterized watermark (text is drawn to a canvas by the UI). */
export interface WatermarkOptions {
  /** PNG/JPEG data URL. */
  imageBase64: string;
  /** 0..1 blend on top of the image alpha. */
  opacity: number;
  /** Width as a fraction of the exported shortest edge. */
  scale: number;
  /** Edge distance as a fraction of the exported shortest edge. */
  margin: number;
  position: WatermarkPosition;
}

export interface ExportOptions {
  path: string;
  format: ExportFormat;
  quality: number;
  maxDimension: number | null;
  width: number | null;
  height: number | null;
  colorSpace: ExportColorSpace;
  metadata: ExportMetadata;
  bitDepth: ExportBitDepth;
  watermark?: WatermarkOptions | null;
}

export interface ExportResult {
  path: string;
  bytes: number;
  width: number;
  height: number;
  durationMs: number;
  backend: string;
  /** Space actually written; may fall back to `sRGB`. */
  colorSpace: string;
  bitDepth: number;
  /** False when the format cannot carry EXIF (TIFF) or the policy forbids it. */
  metadataWritten: boolean;
  keepsLocation: boolean;
}

export interface PortraitRegion {
  kind: string;
  label: string;
  centerX: number;
  centerY: number;
  radiusX: number;
  radiusY: number;
  angle: number;
  feather: number;
  confidence: number;
}

export interface PortraitKeypoint {
  kind: string;
  label: string;
  x: number;
  y: number;
}

export interface PortraitFaceRegions {
  faceId: string;
  box: PortraitFace;
  keypoints: PortraitKeypoint[];
  regions: PortraitRegion[];
  score: number;
}

export interface PortraitRegionsResult {
  available: boolean;
  faces: PortraitFaceRegions[];
  warnings: PortraitWarning[];
}

export interface PortraitWarning {
  code: string;
  message: string;
}

export interface PortraitFace {
  faceId: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** 0..1 ensemble confidence from CenterFace/YuNet. */
  confidence: number;
  /** Which detectors found this face. */
  models?: string[];
}

export interface PortraitAnalysisResult {
  available: boolean;
  faces: PortraitFace[];
  warnings: PortraitWarning[];
}

export type ProviderKind = "mock" | "openaiCompatible";

export interface ProviderCapabilities {
  supportsMask: boolean;
  supportsReferenceImages: boolean;
  supportsMultipleImages: boolean;
  returnsImage: boolean;
  supportsStructuredParameters: boolean;
  supportsCancellation: boolean;
  maxWidth: number | null;
  maxHeight: number | null;
  maxInputBytes: number | null;
}

export interface ProviderInfo {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string | null;
  model: string | null;
  hasCredential: boolean;
  capabilities: ProviderCapabilities;
  createdAt: string;
}

export interface ProviderConfigInput {
  id?: string | null;
  name: string;
  kind: ProviderKind;
  baseUrl?: string | null;
  model?: string | null;
  allowLocalhost?: boolean;
}

export interface ProviderTestResult {
  ok: boolean;
  latencyMs: number;
  message: string;
}

export type AiJobStatus = "queued" | "running" | "completed" | "failed" | "cancelled";

export interface AiJobInfo {
  id: string;
  projectId: string;
  providerId: string;
  providerName: string;
  operation: "edit" | "suggest";
  prompt: string;
  status: AiJobStatus;
  progress: number | null;
  message: string | null;
  /** Asset id of the stored result; compare with LayerState.assetId. */
  resultAssetId: string | null;
  resultPath: string | null;
  errorCode: string | null;
  userMessage: string | null;
  createdAt: string;
  updatedAt: string;
}

/* ------------------------------------------------------------------ */
/* Request payloads (single `req` object for every Tauri command)      */
/* ------------------------------------------------------------------ */

export interface CreateProjectRequest {
  name: string;
}

export interface OpenProjectRequest {
  projectId: string;
}

export interface RenameProjectRequest {
  projectId: string;
  name: string;
}

export interface RenameCollectionRequest {
  collectionId: string;
  name: string;
}

/** Copy the source photo's colour layers onto other photos in the same set. */
export interface SyncColorAdjustmentsRequest {
  sourceProjectId: string;
  projectIds: string[];
}

export interface SyncColorAdjustmentsResult {
  synced: number;
  /** Display names of projects that were skipped or could not be updated. */
  failed: string[];
}

export interface CacheClearResult {
  deletedFiles: number;
  freedBytes: number;
}

export interface ProjectRef {
  projectId: string;
}

export interface ImportPhotoRequest {
  path: string;
  /** Add to an existing project/photo set instead of creating one. */
  collectionId?: string | null;
  /** Name for a newly created project/photo set. */
  collectionName?: string | null;
}

export interface ImportPhotoBytesRequest {
  fileName: string;
  dataBase64: string;
  collectionId?: string | null;
  collectionName?: string | null;
}

export interface RenderPreviewRequest {
  projectId: string;
  maxDimension?: number;
  /** Render the full uncropped frame; used while the crop tool is open. */
  ignoreCrop?: boolean;
}

export interface PickWhiteBalanceRequest {
  projectId: string;
  /** Normalized coordinates in the original (uncropped) source image. */
  x: number;
  y: number;
}

export interface WhiteBalanceResult {
  temperature: number;
  tint: number;
  /** Average sRGB of the sampled neighborhood. */
  sample: [number, number, number];
}

export interface ImportLutRequest {
  projectId: string;
  /** Absolute path from the native file dialog (Tauri only). */
  path?: string | null;
  fileName?: string | null;
  dataBase64?: string | null;
}

export interface LutAssetResult {
  assetId: string;
  displayName: string;
  dimension: "1D" | "3D";
  size: number;
  fileBytes: number;
}

export interface UpdateLayerParamsRequest {
  projectId: string;
  layerId: string;
  color?: ColorParams;
  portrait?: PortraitParams;
  retouch?: RetouchParams;
}

export interface CommitChangesRequest {
  projectId: string;
  label: string;
}

export interface SetCropRequest {
  projectId: string;
  crop: CropState;
}

export interface SetLayerMaskRequest {
  projectId: string;
  layerId: string;
  mask: LayerMask | null;
}

export interface AddLayerRequest {
  projectId: string;
  kind: "portrait" | "color" | "retouch";
  name?: string;
  portrait?: PortraitParams;
  mask?: LayerMask | null;
}

export interface LayerPatch {
  name?: string;
  visible?: boolean;
  locked?: boolean;
  opacity?: number;
  blendMode?: BlendMode;
}

export interface UpdateLayerRequest {
  projectId: string;
  layerId: string;
  patch: LayerPatch;
}

export interface DeleteLayerRequest {
  projectId: string;
  layerId: string;
}

export interface DuplicateLayerRequest {
  projectId: string;
  layerId: string;
}

export interface MoveLayerRequest {
  projectId: string;
  layerId: string;
  direction: "up" | "down";
}

export interface ExportImageRequest {
  projectId: string;
  options: ExportOptions;
  /** Client-generated id for progress events and cancellation. */
  jobId?: string;
}

export interface ExportProgress {
  jobId: string;
  /** "preparing" | "rendering" | "encoding" | "completed" | "cancelled" */
  phase: string;
  percent: number | null;
  message: string;
}

export interface CancelExportRequest {
  jobId: string;
}

export interface SetProviderCredentialRequest {
  providerId: string;
  apiKey: string;
}

export interface ProviderIdRequest {
  providerId: string;
}

export interface SubmitAiJobRequest {
  projectId: string;
  providerId: string;
  operation: "edit" | "suggest";
  prompt: string;
  strength: number;
}

export interface JobIdRequest {
  jobId: string;
}

export interface ApplyAiResultRequest {
  projectId: string;
  jobId: string;
}

export interface AiDenoiseRequest {
  projectId: string;
  /** Absolute path to a user-supplied `.onnx` model (desktop only). */
  modelPath: string;
  /** 0..1 blend between the original and the model output. */
  strength?: number;
}

export interface PhotoHqDenoiseRequest {
  projectId: string;
  /** 0..1 blend between the original and the 4x Photo-HQ output. */
  strength?: number;
  /** Client-generated id used to cancel the running engine. */
  jobId?: string | null;
}

export interface AiEngineCapability {
  available: boolean;
  /** User-facing reason when `available` is false. */
  reason: string | null;
}

export interface AiDenoiseCapability {
  photoHq: AiEngineCapability;
  onnx: AiEngineCapability;
}

export interface AiCapabilityRequest {
  onnxModelPath?: string | null;
}
