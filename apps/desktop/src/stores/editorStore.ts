import { create } from "zustand";
import { getApi, type EditorApi, type EditorMode } from "../api/client";
import { isApiError, userMessageOf } from "../api/errors";
import { trackDocumentWrites } from "../api/saveTracking";
import type {
  AiDenoiseCapability,
  AiJobInfo,
  AppInfo,
  CollectionSummary,
  DocumentState,
  ExportFormat,
  ExportOptions,
  ExportProgress,
  ExportResult,
  Histogram,
  LayerMask,
  LayerPatch,
  LayerState,
  LutAssetResult,
  MaskGradient,
  MaskRegion,
  PortraitFaceRegions,
  LiquifyStroke,
  MaskPoint,
  RetouchMode,
  RetouchSpot,
  MaskStroke,
  PortraitAnalysisResult,
  PortraitGeometry,
  PortraitParams,
  ProjectSummary,
  ProviderConfigInput,
  ProviderInfo,
  ProviderTestResult,
  BlurParams,
  ColorParams,
  SegmentationCapability,
  VersionSummary,
} from "../api/types";
import {
  FULL_CROP,
  aspectRatioOf,
  clampCropToBounds,
  cropBounds,
  draftToRotatedPixels,
  fitCropForAngle,
  pixelsToRotatedDraft,
  rotatedFrameSize,
  type CropAspect,
  type NormalizedCrop,
} from "../lib/crop";
import {
  blobToBase64,
  isSupportedImageFile,
  MAX_IMPORT_BYTES,
  MAX_LUT_BYTES,
  pickImageFile,
  pickImageFiles,
} from "../lib/file";
import { extensionForFormat, renderExportStem } from "../lib/export";
import { buildWatermarkOptions } from "../lib/watermark";
import {
  bestFaceIndex,
  faceLayerName,
  faceMaskRegion,
  hasFineEffects,
  portraitGeometryForFace,
} from "../lib/face";
import { defaultSelection, selectedOrKind, topLayerOfKind } from "../lib/layers";
import { maskHasContent, normalizeMask, regionLabel } from "../lib/mask";
import {
  COLOR_PRESETS,
  DEFAULT_COLOR,
  DEFAULT_PORTRAIT,
  PORTRAIT_EFFECTS,
  PORTRAIT_PRESETS,
  type ColorPresetId,
  type PortraitEffectKey,
  type PortraitPresetId,
} from "../lib/params";
import { createEditQueue } from "./editQueue";
import { useSettingsStore } from "./settingsStore";
import { toastError, toastInfo, toastSuccess } from "./toastStore";

export type ToolId = "photo" | "portrait" | "color" | "crop" | "ai" | "retouch";
export type RetouchToolMode = RetouchMode | "push" | "pinch" | "bulge" | "freeze" | "grid";
export type AutosaveState = "saved" | "saving" | "error";
export type { CropAspect } from "../lib/crop";
export type CropDraft = NormalizedCrop;

export interface PreviewState {
  path: string;
  width: number;
  height: number;
  revision: number;
  fromSource: boolean;
  backend: string;
  histogram: Histogram | null;
}

export type CopiedAdjustments =
  | { kind: "color"; color: ColorParams }
  | { kind: "portrait"; portrait: PortraitParams };

export interface BatchExportState {
  total: number;
  done: number;
  currentName: string;
  failed: number;
}

/** Progress of a multi-photo import; cleared when the last file settles. */
export interface ImportBatchState {
  total: number;
  done: number;
  currentName: string;
}

export /**
 * Longest edge used while a slider is dragged. 900px keeps a full-frame portrait
 * responsive on CPU while still looking sharp at fit-to-window zoom; the
 * configured size (1200/1600/2048) is restored as soon as the drag settles.
 */
const INTERACTIVE_PREVIEW_MAX = 900;
/** Quiet period after the last live edit before the full-size render starts. */
const INTERACTIVE_SETTLE_MS = 420;
/**
 * Quiet period after the last write settles before the status bar says "saved".
 * Without it, every debounce tick during a drag would flash saved/saving.
 */
const SAVED_SETTLE_MS = 400;

const FREEZE_GRID_COLS = 16;
export const FREEZE_GRID_ROWS = 16;
export const FREEZE_GRID_CELLS = FREEZE_GRID_COLS * FREEZE_GRID_ROWS;

function fileNameOfPath(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const index = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return index >= 0 ? trimmed.slice(index + 1) : trimmed;
}

/** Topmost AI denoise layer (ONNX or Photo-HQ) in a document. */
export function latestAiDenoiseLayer(doc: DocumentState | null): LayerState | null {
  if (!doc) return null;
  for (let index = doc.layers.length - 1; index >= 0; index -= 1) {
    const layer = doc.layers[index];
    if (layer.kind === "aiResult" && layer.name.startsWith("AI 降噪")) return layer;
  }
  return null;
}

export interface EditorState {
  api: EditorApi | null;
  mode: EditorMode | null;
  booted: boolean;
  appInfo: AppInfo | null;
  doc: DocumentState | null;
  preview: PreviewState | null;
  previewLoading: boolean;
  recent: ProjectSummary[];
  collections: CollectionSummary[];
  providers: ProviderInfo[];
  jobs: AiJobInfo[];
  selectedLayerId: string | null;
  copiedAdjustments: CopiedAdjustments | null;
  tool: ToolId;
  cropDraft: CropDraft | null;
  cropAspect: CropAspect;
  /** Draft straighten angle for the crop tool, in degrees. */
  cropAngle: number;
  /** Request the uncropped frame while the crop tool is open. */
  previewFullFrame: boolean;
  comparing: boolean;
  splitCompare: boolean;
  maskBrushActive: boolean;
  retouchMode: RetouchToolMode;
  /** Relative to the shorter source side. */
  retouchRadius: number;
  /** 0..1 for liquify strokes. */
  retouchStrength: number;
  /** Clone sample point in normalized source coordinates. */
  retouchSource: MaskPoint | null;
  maskBrushRadius: number;
  maskBrushErase: boolean;
  whiteBalancePickActive: boolean;
  whiteBalancePicking: boolean;
  lastWhiteBalanceSample: [number, number, number] | null;
  zoom: number;
  fitPercent: number;
  autosave: AutosaveState;
  importBusy: boolean;
  /** Set while several photos are imported in one picker action. */
  importBatch: ImportBatchState | null;
  sampleBusy: boolean;
  lutBusy: boolean;
  /** Detected facial regions for the open document, for the overlay and masks. */
  faceRegions: PortraitFaceRegions[];
  /** Face detection result for the selected face and the multi-face list. */
  faceAnalysis: PortraitAnalysisResult | null;
  faceAnalysisDocId: string | null;
  /** Detection ids deliberately hidden from selection and canvas overlays. */
  ignoredFaceIds: string[];
  faceBusy: boolean;
  faceError: string | null;
  selectedFaceIndex: number;
  faceLayerBusy: boolean;
  regionBusy: boolean;
  regionOverlayActive: boolean;
  /** True while measured ellipses are being dragged on the canvas. */
  geometryEditActive: boolean;
  /** Probe result for the segmentation model configured in settings. */
  segmentationCapability: SegmentationCapability | null;
  /** Model path the cached probe describes, so a change re-probes. */
  segmentationCapabilityKey: string | null;
  segmentationBusy: boolean;
  /** True while a local ONNX denoise pass is running. */
  aiDenoiseBusy: boolean;
  /** True while the Photo-HQ 4x pass is running. */
  photoHqBusy: boolean;
  /** Client job id of the running Photo-HQ pass, for cancellation. */
  photoHqJobId: string | null;
  /** Result of the hardware/model probe; null until first requested. */
  aiCapability: AiDenoiseCapability | null;
  aiCapabilityKey: string | null;
  aiCapabilityBusy: boolean;
  /** Session-only labels for imported cube assets; the document stores only ids. */
  lutAssets: Record<string, { name: string; dimension: string; size: number }>;
  exportOpen: boolean;
  exportJob: ExportProgress | null;
  settingsOpen: boolean;
  /** Photos checked in the bottom photo-set strip for batch export. */
  projectSelection: string[];
  batchExport: BatchExportState | null;
  /** True while the selected photos are receiving the current colour stack. */
  syncAdjustmentsBusy: boolean;
  newProjectOpen: boolean;
  newProjectBusy: boolean;
  /** Named snapshots of the open project, newest first. */
  versions: VersionSummary[];
  versionsOpen: boolean;
  versionsBusy: boolean;
  /** Snapshot currently shown side-by-side with the live edit. */
  versionCompare: {
    version: VersionSummary;
    path: string;
    width: number;
    height: number;
  } | null;

  bootstrap: () => Promise<void>;
  refreshRecent: () => Promise<void>;
  refreshCollections: () => Promise<void>;
  refreshProviders: () => Promise<void>;
  openPhoto: () => Promise<void>;
  /** Import local files dropped in the browser demo (validates size/format). */
  importFiles: (files: File[]) => Promise<void>;
  loadSample: () => Promise<void>;
  openProject: (projectId: string) => Promise<void>;
  openCollection: (collectionId: string) => Promise<void>;
  openNewProject: () => void;
  closeNewProject: () => void;
  /** Ask for a name and a photo, then create a new project/photo set. */
  createProjectFromPhoto: (name: string) => Promise<boolean>;
  /** Create a photo set immediately and open it; photos can be added later. */
  createEmptyProject: (name: string) => Promise<boolean>;
  /** Import local files dropped on the desktop window (native paths). */
  importPaths: (paths: string[]) => Promise<void>;
  importBase64: (fileName: string, dataBase64: string) => Promise<void>;
  renameProject: (projectId: string, name: string) => Promise<boolean>;
  renameCollection: (collectionId: string, name: string) => Promise<boolean>;
  deleteProject: (projectId: string) => Promise<boolean>;
  clearRenderCache: () => Promise<{ deletedFiles: number; freedBytes: number } | null>;

  /** Open the versions dialog and load the snapshot list. */
  openVersions: () => Promise<void>;
  closeVersions: () => void;
  /** Capture the current document as a named snapshot. */
  createVersion: (name: string) => Promise<boolean>;
  /** Restore a snapshot; the restore itself becomes one undo step. */
  restoreVersion: (versionId: string) => Promise<void>;
  deleteVersion: (versionId: string) => Promise<void>;
  /** Render a snapshot for side-by-side comparison with the current edit. */
  compareVersion: (versionId: string) => Promise<void>;
  clearVersionCompare: () => void;

  setTool: (tool: ToolId) => void;
  setCropDraft: (draft: CropDraft) => void;
  setCropAspect: (aspect: CropAspect) => void;
  setCropAngle: (angle: number) => void;
  applyCrop: () => Promise<void>;
  resetCrop: () => Promise<void>;
  cancelCrop: () => void;
  selectLayer: (layerId: string | null) => void;
  setComparing: (comparing: boolean) => void;
  setSplitCompare: (enabled: boolean) => void;
  setMaskBrushActive: (enabled: boolean) => void;
  setRetouchMode: (mode: RetouchToolMode) => void;
  setRetouchRadius: (radius: number) => void;
  setRetouchStrength: (strength: number) => void;
  setRetouchSource: (point: MaskPoint | null) => void;
  /** Stamp one clone/heal spot onto the project's retouch layer. */
  addRetouchSpot: (spot: RetouchSpot) => Promise<void>;
  /** Append one liquify stroke to the project's retouch layer. */
  addLiquifyStroke: (stroke: LiquifyStroke) => Promise<void>;
  /** Paint one protection stroke so liquify leaves that area still. */
  addFreezeStroke: (stroke: MaskStroke) => Promise<void>;
  /** Remove every protection stroke from the retouch layer. */
  clearFreezeStrokes: () => Promise<void>;
  /** Toggle one 16x16 grid cell on the full frame as warp protection. */
  toggleFreezeGridCell: (index: number) => Promise<void>;
  ensureRetouchLayer: () => Promise<LayerState | null>;
  setMaskBrushRadius: (radius: number) => void;
  setMaskBrushErase: (erase: boolean) => void;
  setWhiteBalancePickActive: (active: boolean) => void;
  pickWhiteBalance: (point: MaskPoint) => Promise<void>;
  applyAutoTone: () => Promise<void>;
  detectFaces: () => Promise<void>;
  setSelectedFaceIndex: (index: number) => void;
  /** Hide or restore one detected face; at least one face stays available. */
  toggleFaceIgnored: (faceId: string) => void;
  /** Create or reuse the portrait layer dedicated to the selected face. */
  ensureFacePortraitLayer: (overrides?: Partial<PortraitParams>) => Promise<LayerState | null>;
  /** Create or reuse the body-beauty layer (body skin mask, no face mask). */
  ensureBodyPortraitLayer: () => Promise<LayerState | null>;
  analyzeFaceRegions: (options?: { silent?: boolean }) => Promise<void>;
  setRegionOverlayActive: (active: boolean) => void;
  /** Enter/leave manual keypoint correction; entering ensures the face layer. */
  setGeometryEditActive: (active: boolean) => Promise<void>;
  /** Apply manually dragged ellipses to the selected face's portrait layer. */
  updateFaceGeometry: (geometry: PortraitGeometry) => void;
  /** Run the local ONNX denoiser and add its result as an AI layer. */
  runAiDenoise: (strength?: number) => Promise<LayerState | null>;
  /** Run the bundled Photo-HQ-W4xEX 4x model and add its result as a layer. */
  runPhotoHqDenoise: (strength?: number) => Promise<LayerState | null>;
  /** Ask the running Photo-HQ engine to stop; no layer is added. */
  cancelPhotoHqDenoise: () => Promise<void>;
  /** Probe Photo-HQ (real inference) and the configured ONNX model. */
  checkAiCapability: () => Promise<AiDenoiseCapability>;
  /**
   * Turn an existing AI layer on/off, or create one when needed. Returns
   * `ok: false` with a reason when no AI engine can run on this machine.
   */
  setAiDenoiseEnabled: (
    enabled: boolean,
    strength: number,
  ) => Promise<{
    ok: boolean;
    engine: "photoHq" | "onnx" | null;
    reason: string | null;
    /** True when the user switched projects while the check/run was in flight. */
    projectChanged?: boolean;
  }>;
  addMaskRegion: (region: MaskRegion) => Promise<void>;
  removeMaskRegion: (layerId: string, index: number) => Promise<void>;
  importLut: (file: File | null) => Promise<void>;
  removeLut: () => Promise<void>;
  addMaskStroke: (layerId: string, stroke: MaskStroke) => Promise<void>;
  setAutoSkinMask: (layerId: string, enabled: boolean) => Promise<void>;
  /** Probe the configured segmentation model (cached per path). */
  checkSegmentation: () => Promise<void>;
  /** Run the model and attach its output as a raster mask on the layer. */
  applySegmentationMask: (layerId: string) => Promise<void>;
  /** Remove only the raster coverage, keeping brush and gradient sources. */
  clearRasterMask: (layerId: string) => Promise<void>;
  setMaskGradient: (layerId: string, gradient: MaskGradient | null) => Promise<void>;
  setMaskOptions: (
    layerId: string,
    options: Partial<Pick<LayerMask, "inverted" | "feather" | "density">>,
  ) => Promise<void>;
  clearLayerMask: (layerId: string) => Promise<void>;
  setZoom: (zoom: number) => void;
  setFitPercent: (percent: number) => void;
  openExport: () => void;
  closeExport: () => void;
  openSettings: () => void;
  closeSettings: () => void;
  refreshPreview: () => void;
  toggleProjectSelection: (projectId: string) => void;
  selectAllProjects: () => void;
  clearProjectSelection: () => void;
  /** Choose a folder and export every selected project with current defaults. */
  exportSelectedProjects: () => Promise<void>;
  /** Copy the current photo's colour layers onto the selected photos. */
  syncColorAdjustmentsToSelection: () => Promise<void>;

  undo: () => Promise<void>;
  redo: () => Promise<void>;

  addLayer: (
    kind: "portrait" | "color" | "retouch" | "blur",
    options?: { name?: string; portrait?: PortraitParams; mask?: LayerMask | null },
  ) => Promise<LayerState | null>;
  ensureColorLayer: () => Promise<LayerState | null>;
  duplicateLayer: (layerId: string) => Promise<void>;
  deleteLayer: (layerId: string) => Promise<void>;
  copyLayerAdjustments: (layerId: string) => void;
  pasteLayerAdjustments: () => Promise<void>;
  moveLayer: (layerId: string, direction: "up" | "down") => Promise<void>;
  setLayerVisibility: (layerId: string, visible: boolean) => Promise<void>;
  setLayerBlendMode: (layerId: string, blendMode: LayerState["blendMode"]) => Promise<void>;
  setLayerOpacity: (layerId: string, opacity: number) => void;

  editPortrait: (layerId: string, params: PortraitParams) => void;
  editColor: (layerId: string, params: ColorParams) => void;
  /** Live radius change for a background-blur layer. */
  editBlur: (layerId: string, params: BlurParams) => void;
  commitEdit: (label: string) => Promise<void>;
  /** Flush queued edits and commit before the window closes. */
  flushPendingEdits: () => Promise<boolean>;
  /** Re-attempt the save that put the indicator into the error state. */
  retrySave: () => Promise<void>;
  commitPortrait: (layerId: string, params: PortraitParams, label: string) => Promise<void>;
  commitColor: (layerId: string, params: ColorParams, label: string) => Promise<void>;
  /**
   * One-click look on top of an existing mask: a new colour layer reusing the
   * selected layer's mask (raster included) and a preset, committed as one step.
   */
  applyLook: (
    layerId: string,
    look: "brightenSubject" | "desaturateBackground",
  ) => Promise<void>;
  applyPortraitPreset: (presetId: PortraitPresetId) => Promise<void>;
  applyColorPreset: (presetId: ColorPresetId) => Promise<void>;
  setPortraitEffect: (layerId: string, key: PortraitEffectKey, enabled: boolean) => Promise<void>;
  resetPortrait: (layerId: string) => Promise<void>;
  resetColor: (layerId: string) => Promise<void>;

  loadJobs: () => Promise<void>;
  saveProviderConfig: (input: ProviderConfigInput, apiKey: string) => Promise<ProviderInfo | null>;
  testProvider: (providerId: string) => Promise<ProviderTestResult | null>;
  listProviderModels: (providerId: string) => Promise<string[] | null>;
  submitAiJob: (providerId: string, prompt: string, strength: number) => Promise<string | null>;
  cancelAiJob: (jobId: string) => Promise<void>;
  applyAiJob: (jobId: string) => Promise<void>;
  receiveAiJob: (job: AiJobInfo) => void;

  chooseExportPath: (defaultName: string, format: ExportFormat) => Promise<string | null>;
  exportPhoto: (options: ExportOptions) => Promise<ExportResult>;
  cancelExport: () => Promise<void>;
  receiveExportProgress: (progress: ExportProgress) => void;
}

let bootstrapPromise: Promise<void> | null = null;

export const useEditorStore = create<EditorState>()((set, get) => {
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
  let previewRunning = false;
  let previewDirty = false;
  let previewEpoch = 0;
  /**
   * True while a slider drag is being previewed. The canvas renders a smaller
   * proxy during the drag and always returns to the configured size afterwards,
   * including when the drag ends without a commit.
   */
  let interactivePreview = false;
  let interactiveTimer: ReturnType<typeof setTimeout> | null = null;
  let autosaveTimer: ReturnType<typeof setTimeout> | null = null;
  /** Document writes currently in flight, counted by the tracked API wrapper. */
  let pendingWrites = 0;

  let colorCreation: Promise<LayerState | null> | null = null;

  function errorToast(error: unknown, fallback: string): void {
    toastError(userMessageOf(error, fallback));
  }

  /** Async actions must not write their result into a project opened later. */
  function isCurrentProject(projectId: string): boolean {
    return get().doc?.id === projectId;
  }

  function clearAutosaveTimer(): void {
    if (autosaveTimer !== null) {
      clearTimeout(autosaveTimer);
      autosaveTimer = null;
    }
  }

  /** Called by the tracked API when a document write leaves the renderer. */
  function markWriteStart(): void {
    pendingWrites += 1;
    clearAutosaveTimer();
    if (get().autosave !== "saving") set({ autosave: "saving" });
  }

  /**
   * Called when a write settles. "saved" only appears once every in-flight
   * write finished and a short quiet period passed, so a slow disk can no longer
   * show a premature success (the previous 900 ms timer could, because it was
   * started before the write resolved).
   */
  function markWriteSettled(ok: boolean): void {
    pendingWrites = Math.max(0, pendingWrites - 1);
    if (!ok) {
      markSaveFailed();
      return;
    }
    if (pendingWrites > 0) return;
    clearAutosaveTimer();
    autosaveTimer = setTimeout(() => {
      autosaveTimer = null;
      if (pendingWrites > 0) return;
      // A failure stays visible until a later write succeeds again.
      if (get().autosave === "error") return;
      set({ autosave: "saved" });
    }, SAVED_SETTLE_MS);
  }

  /** A write failed: stop pretending it was saved until one succeeds again. */
  function markSaveFailed(): void {
    pendingWrites = 0;
    clearAutosaveTimer();
    set({ autosave: "error" });
  }

  function patchLocalLayer(layerId: string, updater: (layer: LayerState) => LayerState): void {
    set((state) => {
      if (!state.doc) return {};
      let changed = false;
      const layers = state.doc.layers.map((layer) => {
        if (layer.id !== layerId) return layer;
        changed = true;
        return updater(layer);
      });
      if (!changed) return {};
      return { doc: { ...state.doc, layers } };
    });
  }

  function mergeServerMeta(next: DocumentState): void {
    set((state) => {
      if (!state.doc || state.doc.id !== next.id) return {};
      return {
        doc: {
          ...state.doc,
          revision: next.revision,
          updatedAt: next.updatedAt,
          canUndo: next.canUndo,
          canRedo: next.canRedo,
          history: next.history,
        },
      };
    });
  }

  /**
   * Merge only server-owned layer metadata after a discrete patch. Replacing the
   * whole document here would overwrite parameter edits that are still queued.
   */
  function mergeServerLayer(next: DocumentState, layerId: string): void {
    set((state) => {
      if (!state.doc || state.doc.id !== next.id) return {};
      const serverLayer = next.layers.find((layer) => layer.id === layerId);
      if (!serverLayer) return {};
      return {
        doc: {
          ...state.doc,
          revision: next.revision,
          updatedAt: next.updatedAt,
          canUndo: next.canUndo,
          canRedo: next.canRedo,
          history: next.history,
          layers: state.doc.layers.map((layer) =>
            layer.id === layerId
              ? {
                  ...layer,
                  name: serverLayer.name,
                  visible: serverLayer.visible,
                  locked: serverLayer.locked,
                  opacity: serverLayer.opacity,
                  blendMode: serverLayer.blendMode,
                }
              : layer,
          ),
        },
      };
    });
  }

  function portraitLayerForFace(doc: DocumentState | null, faceIndex: number): LayerState | null {
    return (
      doc?.layers.find(
        (layer) => layer.kind === "portrait" && layer.portrait?.faceIndex === faceIndex,
      ) ?? null
    );
  }

  /**
   * Geometry for a portrait layer, preferring what the layer already stores.
   * Measured regions are used when available; otherwise the face box resolves
   * to proportional feature positions in the backend.
   */
  function geometryForLayer(layer: LayerState): PortraitGeometry | null {
    if (layer.portrait?.geometry) return layer.portrait.geometry;
    const state = get();
    const faces = state.faceAnalysis?.faces ?? [];
    if (faces.length === 0) return null;
    const stored = layer.portrait?.faceIndex;
    const index =
      stored != null && stored < faces.length
        ? stored
        : faces.length === 1
          ? 0
          : state.selectedFaceIndex;
    const face = faces[index];
    if (!face) return null;
    const measured = state.faceRegions.find((item) => item.faceId === face.faceId) ?? null;
    const width = state.doc?.source?.width ?? state.doc?.canvas.width ?? 1;
    const height = state.doc?.source?.height ?? state.doc?.canvas.height ?? 1;
    return portraitGeometryForFace(face, width, height, measured);
  }

  function applyDocument(next: DocumentState): void {
    const selectedLayerId = get().selectedLayerId;
    const stillExists = next.layers.some((layer) => layer.id === selectedLayerId);
    set({
      doc: next,
      selectedLayerId: stillExists
        ? selectedLayerId
        : defaultSelection(next, get().tool === "color" ? "color" : "portrait"),
    });
  }

  function schedulePreview(delay = 100): void {
    previewDirty = true;
    previewEpoch += 1;
    if (previewRunning || previewTimer !== null) return;
    previewTimer = setTimeout(() => {
      previewTimer = null;
      void runPreview(previewEpoch);
    }, delay);
  }

  /**
   * A live edit started. Previews drop to the interactive tier so the canvas can
   * follow the pointer; a settle timer guarantees the full-size render even when
   * no commit follows (for example a drag that ends on the same value).
   */
  function noteInteractiveEdit(): void {
    interactivePreview = true;
    if (interactiveTimer !== null) clearTimeout(interactiveTimer);
    interactiveTimer = setTimeout(() => {
      interactiveTimer = null;
      if (!interactivePreview) return;
      interactivePreview = false;
      schedulePreview(30);
    }, INTERACTIVE_SETTLE_MS);
  }

  /** Leave the interactive tier immediately, e.g. when an edit is committed. */
  function endInteractivePreview(): void {
    if (interactiveTimer !== null) {
      clearTimeout(interactiveTimer);
      interactiveTimer = null;
    }
    interactivePreview = false;
  }

  async function runPreview(epoch: number): Promise<void> {
    const api = get().api;
    const doc = get().doc;
    if (!api || !doc || !doc.source) {
      previewDirty = false;
      return;
    }
    previewRunning = true;
    previewDirty = false;
    set({ previewLoading: true });
    const projectId = doc.id;
    const requestRevision = doc.revision;
    try {
      const settings = useSettingsStore.getState();
      const result = await api.renderPreview({
        projectId,
        maxDimension: interactivePreview
          ? Math.min(settings.previewMaxDimension, INTERACTIVE_PREVIEW_MAX)
          : settings.previewMaxDimension,
        ignoreCrop: get().previewFullFrame,
      });
      const current = get().doc;
      const stillSameProject = current?.id === projectId;
      const stale =
        epoch !== previewEpoch ||
        !stillSameProject ||
        result.revision < requestRevision ||
        result.revision < (current?.revision ?? 0);
      if (!stale && current) {
        set({
          preview: {
            path: result.path,
            width: result.width,
            height: result.height,
            revision: result.revision,
            fromSource: false,
            backend: result.backend,
            histogram: result.histogram,
          },
        });
      } else if (stillSameProject) {
        previewDirty = true;
      }
    } catch (error) {
      if (get().doc?.id === projectId) {
        errorToast(error, "预览更新没有完成，画面仍保留上一次的结果。");
      }
    } finally {
      previewRunning = false;
      if (previewDirty) {
        schedulePreview(90);
      } else {
        set({ previewLoading: false });
      }
    }
  }

  // High-frequency edits are coalesced and written by a separate subsystem.
  // These three wrappers keep every existing call site in this file unchanged.
  const editQueue = createEditQueue({
    snapshot: () => ({ api: get().api, doc: get().doc }),
    mergeServerMeta,
    markSaveFailed,
    onError: errorToast,
    schedulePreview,
  });

  function queueParams(
    layerId: string,
    patch: { color?: ColorParams; portrait?: PortraitParams; blur?: BlurParams },
  ): void {
    editQueue.queueParams(layerId, patch);
  }

  function queueLayerPatch(layerId: string, patch: LayerPatch): void {
    editQueue.queueLayerPatch(layerId, patch);
  }

  function flushSync(): Promise<void> {
    return editQueue.flush();
  }

  /**
   * Persist a pending live edit before leaving the current project. Without
   * this, queued slider changes are dropped when the document id changes.
   */
  async function settlePendingEdits(): Promise<boolean> {
    // A commit is about to run (leaving the project or closing the window), so
    // never keep the canvas on the interactive tier.
    endInteractivePreview();
    // Always commit, even with an empty queue: a drag whose updateLayerParams
    // already reached the backend is still uncommitted and therefore unsaved.
    // The backend skips the write when nothing changed, so this is cheap.
    await flushSync();
    const api = get().api;
    const doc = get().doc;
    if (!api || !doc) return true;
    try {
      const next = await api.commitChanges({
        projectId: doc.id,
        label: "自动保存调整",
      });
      mergeServerMeta(next);
      return true;
    } catch (error) {
      markSaveFailed();
      errorToast(error, "离开前的调整没有写入历史，请重试。");
      return false;
    }
  }

  async function importPathBatch(paths: string[]): Promise<void> {
    const api = get().api;
    if (!api || paths.length === 0 || get().importBusy || get().importBatch) return;
    if (!(await settlePendingEdits())) return;
    await importMany(paths.map(fileNameOfPath), (index, collectionId) =>
      api.importPhoto({ path: paths[index], collectionId }),
    );
  }

  async function importFileBatch(files: File[]): Promise<void> {
    const api = get().api;
    if (!api || files.length === 0 || get().importBusy || get().importBatch) return;
    const accepted: File[] = [];
    let skipped = 0;
    for (const file of files) {
      if (file.size > MAX_IMPORT_BYTES || !isSupportedImageFile(file.name, file.type)) {
        skipped += 1;
      } else {
        accepted.push(file);
      }
    }
    if (skipped > 0) {
      toastError(
        `已跳过 ${skipped} 个不支持或超过 32 MB 的文件。`,
        "部分文件未导入",
      );
    }
    if (accepted.length === 0) return;
    if (!(await settlePendingEdits())) return;
    await importMany(accepted.map((file) => file.name), async (index, collectionId) => {
      const file = accepted[index];
      return api.importPhotoBytes({
        fileName: file.name,
        dataBase64: await blobToBase64(file),
        collectionId,
      });
    });
  }

  /**
   * Import a hand-picked list sequentially into the current photo set.
   *
   * One-at-a-time keeps memory bounded for large selections. With no current
   * project the first success creates the set and the rest join it; failures
   * are collected instead of aborting the whole batch.
   */
  async function importMany(
    names: string[],
    importAt: (index: number, collectionId: string | null) => Promise<DocumentState>,
  ): Promise<void> {
    const existingCollection = get().doc?.collectionId ?? null;
    let collectionId = existingCollection;
    let first: DocumentState | null = null;
    let done = 0;
    const failed: string[] = [];
    set({
      importBusy: true,
      importBatch: { total: names.length, done: 0, currentName: names[0] ?? "" },
    });
    try {
      for (let index = 0; index < names.length; index += 1) {
        set((state) =>
          state.importBatch
            ? { importBatch: { ...state.importBatch, currentName: names[index] } }
            : {},
        );
        try {
          const next = await importAt(index, collectionId);
          collectionId ??= next.collectionId;
          first ??= next;
          done += 1;
        } catch {
          failed.push(names[index]);
        }
        set((state) =>
          state.importBatch ? { importBatch: { ...state.importBatch, done } } : {},
        );
      }
      if (!existingCollection && first) activateDocument(first);
      await get().refreshRecent();
      const target = existingCollection ? "当前项目" : "新照片集";
      if (failed.length === 0) {
        toastSuccess(
          `${done} 张照片已加入${target}。`,
          done === 1 ? "照片已导入" : "批量导入完成",
        );
      } else {
        const failedText = failed.slice(0, 3).join("、");
        toastError(
          `已导入 ${done} 张，${failed.length} 张失败：${failedText}${failed.length > 3 ? " 等" : ""}`,
          "批量导入未全部完成",
        );
      }
    } finally {
      set({ importBusy: false, importBatch: null });
    }
  }

  async function commitLabel(label: string): Promise<void> {    await flushSync();
    const api = get().api;
    const doc = get().doc;
    if (!api || !doc) return;
    try {
      const next = await api.commitChanges({ projectId: doc.id, label });
      applyDocument(next);
      schedulePreview(50);
    } catch (error) {
      markSaveFailed();
      errorToast(error, "这次调整没有写入历史，请重试。");
    }
  }

  async function finishEdit(label: string): Promise<void> {
    // The drag is over: the commit below requests the full-size preview.
    endInteractivePreview();
    if (colorCreation) {
      try {
        await colorCreation;
      } catch {
        // The creation flow already showed a friendly error.
      }
    }
    await commitLabel(label);
  }

  async function persistLayerMask(
    layerId: string,
    nextMask: LayerMask | null,
  ): Promise<void> {
    const api = get().api;
    const doc = get().doc;
    const layer = doc?.layers.find((item) => item.id === layerId);
    if (!api || !doc || !layer) return;
    const previous = layer.mask;
    patchLocalLayer(layerId, (item) => ({ ...item, mask: nextMask }));
    await flushSync();
    try {
      const next = await api.setLayerMask({ projectId: doc.id, layerId, mask: nextMask });
      applyDocument(next);
      schedulePreview(40);
    } catch (error) {
      patchLocalLayer(layerId, (item) => ({ ...item, mask: previous }));
      markSaveFailed();
      errorToast(error, "修饰范围没有保存，请重试。");
    }
  }

  function activateDocument(next: DocumentState): void {
    endInteractivePreview();
    set((state) => {
      // LUT names are session-only. Keep only the ids the new document still
      // references so importing many LUTs across projects cannot grow forever.
      const referenced = new Set<string>();
      for (const layer of next.layers) {
        if (layer.color?.lutAssetId) referenced.add(layer.color.lutAssetId);
      }
      const lutAssets = Object.fromEntries(
        Object.entries(state.lutAssets).filter(([assetId]) => referenced.has(assetId)),
      );
      return {
        doc: next,
        selectedLayerId: defaultSelection(next, "portrait"),
        preview: next.source
          ? {
              path: next.source.previewPath,
              width: next.canvas.width,
              height: next.canvas.height,
              revision: next.revision,
              fromSource: true,
              backend: "source",
              histogram: null,
            }
          : null,
        previewLoading: false,
        comparing: false,
        splitCompare: false,
        maskBrushActive: false,
        retouchMode: "heal",
        retouchRadius: 0.03,
        retouchStrength: 0.5,
        retouchSource: null,
        maskBrushErase: false,
        whiteBalancePickActive: false,
        whiteBalancePicking: false,
        lastWhiteBalanceSample: null,
        faceRegions: [],
        faceAnalysis: null,
        faceAnalysisDocId: null,
        ignoredFaceIds: [],
        geometryEditActive: false,
        faceBusy: false,
        faceError: null,
        selectedFaceIndex: 0,
        faceLayerBusy: false,
        regionOverlayActive: false,
        regionBusy: false,
        lutBusy: false,
        zoom: 1,
        jobs: [],
        autosave: "saved",
        exportOpen: false,
        exportJob: null,
        tool: next.source ? "portrait" : "photo",
        cropDraft: null,
        cropAspect: "free",
        cropAngle: 0,
        previewFullFrame: false,
        lutAssets,
        projectSelection:
          state.doc?.collectionId === next.collectionId ? state.projectSelection : [],
        // Snapshots belong to the project that captured them.
        versions: [],
        versionsOpen: false,
        versionsBusy: false,
        versionCompare: null,
      };
    });
    schedulePreview(20);
    void get().loadJobs();
  }

  return {
    api: null,
    mode: null,
    booted: false,
    appInfo: null,
    doc: null,
    preview: null,
    previewLoading: false,
    recent: [],
    collections: [],
    providers: [],
    jobs: [],
    selectedLayerId: null,
    copiedAdjustments: null,
    tool: "portrait",
    cropDraft: null,
    cropAspect: "free",
    cropAngle: 0,
    previewFullFrame: false,
    comparing: false,
    splitCompare: false,
    maskBrushActive: false,
    maskBrushRadius: 0.06,
    maskBrushErase: false,
    retouchMode: "heal",
    retouchRadius: 0.03,
    retouchStrength: 0.5,
    retouchSource: null,
    whiteBalancePickActive: false,
    whiteBalancePicking: false,
    lastWhiteBalanceSample: null,
    zoom: 1,
    fitPercent: 0,
    autosave: "saved",
    importBusy: false,
    importBatch: null,
    sampleBusy: false,
    lutBusy: false,
    faceRegions: [],
    faceAnalysis: null,
    faceAnalysisDocId: null,
    ignoredFaceIds: [],
    faceBusy: false,
    faceError: null,
    selectedFaceIndex: 0,
    faceLayerBusy: false,
    regionBusy: false,
    regionOverlayActive: false,
    geometryEditActive: false,
    segmentationCapability: null,
    segmentationCapabilityKey: null,
    segmentationBusy: false,
    aiDenoiseBusy: false,
    photoHqBusy: false,
    photoHqJobId: null,
    aiCapability: null,
    aiCapabilityKey: null,
    aiCapabilityBusy: false,
    lutAssets: {},
    exportOpen: false,
    exportJob: null,
    settingsOpen: false,
    projectSelection: [],
    batchExport: null,
    syncAdjustmentsBusy: false,
    newProjectOpen: false,
    newProjectBusy: false,
    versions: [],
    versionsOpen: false,
    versionsBusy: false,
    versionCompare: null,

    bootstrap: async () => {
      if (bootstrapPromise) return bootstrapPromise;
      bootstrapPromise = (async () => {
        // Wrapping here is what makes the status bar truthful: the indicator is
        // driven by write completion, not by a timer started before the write.
        const api = trackDocumentWrites(getApi(), {
          start: markWriteStart,
          settle: markWriteSettled,
        });
        set({ api, mode: api.mode });
        const [infoResult, projectsResult, collectionsResult, providersResult] =
          await Promise.allSettled([
            api.getAppInfo(),
            api.listProjects(),
            api.listCollections(),
            api.listProviders(),
          ]);
        set({
          appInfo: infoResult.status === "fulfilled" ? infoResult.value : null,
          recent: projectsResult.status === "fulfilled" ? projectsResult.value : [],
          collections:
            collectionsResult.status === "fulfilled" ? collectionsResult.value : [],
          providers: providersResult.status === "fulfilled" ? providersResult.value : [],
          booted: true,
        });
        if (infoResult.status === "rejected") {
          errorToast(infoResult.reason, "应用信息暂时不可用，你仍然可以打开照片。");
        }
      })().catch((error: unknown) => {
        set({ booted: true });
        errorToast(error, "编辑器初始化没有完成，请刷新后重试。");
      });
      return bootstrapPromise;
    },

    refreshRecent: async () => {
      const api = get().api;
      if (!api) return;
      try {
        const [recent, collections] = await Promise.all([
          api.listProjects(),
          api.listCollections(),
        ]);
        set({ recent, collections });
      } catch {
        // Recent projects are auxiliary; keep the current list on failure.
      }
    },

    refreshCollections: async () => {
      const api = get().api;
      if (!api) return;
      try {
        set({ collections: await api.listCollections() });
      } catch {
        // Collections are auxiliary; keep the current list on failure.
      }
    },

    refreshProviders: async () => {
      const api = get().api;
      if (!api) return;
      try {
        const providers = await api.listProviders();
        set({ providers });
      } catch {
        // The AI panel surfaces an empty state; avoid noisy toasts on boot.
      }
    },

    openPhoto: async () => {
      if (get().importBusy || get().importBatch) return;
      const api = get().api;
      try {
        if (get().mode === "tauri") {
          if (!api) return;
          await importPathBatch(await api.pickImages());
          return;
        }
        await importFileBatch(await pickImageFiles());
      } catch (error) {
        errorToast(error, "无法打开文件选择窗口，请重试。");
      }
    },

    importPaths: async (paths) => {
      await importPathBatch(paths);
    },

    importFiles: async (files) => {
      await importFileBatch(files);
    },

    loadSample: async () => {
      if (get().sampleBusy || get().importBusy) return;
      set({ sampleBusy: true });
      try {
        const response = await fetch("/samples/portrait.jpg");
        if (!response.ok) throw new Error("SAMPLE_FETCH_FAILED");
        const blob = await response.blob();
        const base64 = await blobToBase64(blob);
        await get().importBase64("示例人像.jpg", base64);
      } catch (error) {
        errorToast(error, "示例照片暂时无法载入，请直接打开本地照片。");
      } finally {
        set({ sampleBusy: false });
      }
    },

    openProject: async (projectId: string) => {
      const api = get().api;
      if (!api || get().importBusy) return;
      set({ importBusy: true });
      try {
        await settlePendingEdits();
        const next = await api.openProject({ projectId });
        activateDocument(next);
        await get().refreshRecent();
      } catch (error) {
        errorToast(error, "这个项目暂时无法打开，请重新导入照片。");
      } finally {
        set({ importBusy: false });
      }
    },

    openCollection: async (collectionId: string) => {
      const api = get().api;
      if (!api) return;
      const collection = get().collections.find((item) => item.id === collectionId);
      const target =
        collection?.latestProjectId ??
        get().recent.find((project) => project.collectionId === collectionId)?.id ??
        null;
      if (!target) {
        await get().refreshRecent();
        toastError("这个项目里还没有照片，请先导入照片。", "项目为空");
        return;
      }
      await get().openProject(target);
    },

    openNewProject: () =>
      set({
        newProjectOpen: true,
        exportOpen: false,
        settingsOpen: false,
        versionsOpen: false,
      }),

    closeNewProject: () => {
      if (!get().newProjectBusy) set({ newProjectOpen: false });
    },

    openVersions: async () => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      set({
        versionsOpen: true,
        versionsBusy: true,
        exportOpen: false,
        settingsOpen: false,
      });
      try {
        const versions = await api.listVersions({ projectId: doc.id });
        if (isCurrentProject(doc.id)) set({ versions });
      } catch (error) {
        errorToast(error, "版本列表没有加载出来，请稍后重试。");
      } finally {
        set({ versionsBusy: false });
      }
    },

    closeVersions: () => {
      if (!get().versionsBusy) set({ versionsOpen: false, versionCompare: null });
    },

    createVersion: async (name) => {
      const api = get().api;
      const doc = get().doc;
      const label = name.trim();
      if (!api || !doc || !label) return false;
      set({ versionsBusy: true });
      try {
        const versions = await api.createVersion({ projectId: doc.id, name: label });
        if (isCurrentProject(doc.id)) set({ versions });
        toastSuccess(`已保存版本「${label}」，重新打开项目后仍可恢复。`);
        return true;
      } catch (error) {
        errorToast(error, "版本没有保存成功，请重试。");
        return false;
      } finally {
        set({ versionsBusy: false });
      }
    },

    restoreVersion: async (versionId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      // Apply queued slider edits first: otherwise the restore would be based on
      // a document that still has uncommitted parameters, and "undo" would return
      // to that half-applied state.
      await flushSync();
      set({ versionsBusy: true });
      try {
        const next = await api.restoreVersion({ projectId: doc.id, versionId });
        if (isCurrentProject(doc.id)) {
          applyDocument(next);
          schedulePreview(40);
          toastSuccess("已恢复这个版本，可用撤销回到恢复前的状态。");
        }
      } catch (error) {
        errorToast(error, "这个版本没有恢复成功，请重试。");
      } finally {
        set({ versionsBusy: false });
      }
    },

    deleteVersion: async (versionId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      set({ versionsBusy: true });
      try {
        const versions = await api.deleteVersion({ projectId: doc.id, versionId });
        if (isCurrentProject(doc.id)) {
          const comparing = get().versionCompare;
          set({
            versions,
            // The rendered file is gone with the snapshot.
            versionCompare: comparing?.version.id === versionId ? null : comparing,
          });
        }
      } catch (error) {
        errorToast(error, "删除版本失败，请重试。");
      } finally {
        set({ versionsBusy: false });
      }
    },

    compareVersion: async (versionId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      const version = get().versions.find((item) => item.id === versionId);
      if (!version) return;
      set({ versionsBusy: true });
      try {
        const preview = await api.renderVersionPreview({
          projectId: doc.id,
          versionId,
          maxDimension: useSettingsStore.getState().previewMaxDimension,
        });
        if (isCurrentProject(doc.id)) {
          set({
            versionCompare: {
              version,
              path: preview.path,
              width: preview.width,
              height: preview.height,
            },
          });
        }
      } catch (error) {
        errorToast(error, "这个版本的对比图没有生成，请重试。");
      } finally {
        set({ versionsBusy: false });
      }
    },

    clearVersionCompare: () => set({ versionCompare: null }),

    createEmptyProject: async (name) => {
      const api = get().api;
      const projectName = name.trim();
      if (!api || get().newProjectBusy) return false;
      if (!projectName) {
        toastError("请先给新项目起一个名字。", "需要项目名称");
        return false;
      }
      set({ newProjectBusy: true });
      try {
        await settlePendingEdits();
        const next = await api.createProject({ name: projectName });
        activateDocument(next);
        await Promise.all([get().refreshRecent(), get().refreshCollections()]);
        set({ newProjectOpen: false });
        toastSuccess("空照片集已创建，可随时导入照片。", "已创建");
        return true;
      } catch (error) {
        errorToast(error, "没有成功创建项目，请重试。");
        return false;
      } finally {
        set({ newProjectBusy: false });
      }
    },

    createProjectFromPhoto: async (name: string) => {
      const api = get().api;
      const projectName = name.trim();
      if (!api || get().newProjectBusy) return false;
      if (!projectName) {
        toastError("请先给新项目起一个名字。", "需要项目名称");
        return false;
      }
      set({ newProjectBusy: true });
      try {
        await settlePendingEdits();
        let next: DocumentState | null = null;
        if (get().mode === "tauri") {
          const path = await api.pickImage();
          if (!path) return false;
          next = await api.importPhoto({ path, collectionName: projectName });
        } else {
          const file = await pickImageFile();
          if (!file) return false;
          next = await api.importPhotoBytes({
            fileName: file.name,
            dataBase64: await blobToBase64(file),
            collectionName: projectName,
          });
        }
        activateDocument(next);
        await get().refreshRecent();
        set({ newProjectOpen: false });
        toastSuccess(`「${projectName}」已创建，可继续在底部添加照片。`, "项目已创建");
        return true;
      } catch (error) {
        errorToast(error, "新项目没有创建完成，请重试。");
        return false;
      } finally {
        set({ newProjectBusy: false });
      }
    },

    renameProject: async (projectId, name) => {
      const api = get().api;
      if (!api) return false;
      try {
        const summary = await api.renameProject({ projectId, name });
        set((state) => ({
          doc:
            state.doc && state.doc.id === projectId
              ? { ...state.doc, name: summary.name }
              : state.doc,
          recent: state.recent.map((item) => (item.id === projectId ? summary : item)),
        }));
        toastSuccess("项目名称已更新。", "已重命名");
        return true;
      } catch (error) {
        errorToast(error, "没有成功重命名项目，请重试。");
        return false;
      }
    },

    renameCollection: async (collectionId, name) => {
      const api = get().api;
      if (!api) return false;
      try {
        await api.renameCollection({ collectionId, name });
        set((state) => ({
          doc:
            state.doc && state.doc.collectionId === collectionId
              ? { ...state.doc, collectionName: name.trim() }
              : state.doc,
          collections: state.collections.map((item) =>
            item.id === collectionId ? { ...item, name: name.trim() } : item,
          ),
          recent: state.recent.map((item) =>
            item.collectionId === collectionId ? { ...item, collectionName: name.trim() } : item,
          ),
        }));
        void get().refreshCollections();
        toastSuccess("照片集名称已更新。", "已重命名");
        return true;
      } catch (error) {
        errorToast(error, "没有成功重命名照片集，请重试。");
        return false;
      }
    },

    deleteProject: async (projectId) => {
      const api = get().api;
      if (!api) return false;
      try {
        await api.deleteProject({ projectId });
        if (get().doc?.id === projectId) {
          set({
            doc: null,
            preview: null,
            previewLoading: false,
            selectedLayerId: null,
            jobs: [],
            comparing: false,
            splitCompare: false,
            autosave: "saved",
            faceRegions: [],
            faceAnalysis: null,
            faceAnalysisDocId: null,
            ignoredFaceIds: [],
            faceBusy: false,
            faceError: null,
            selectedFaceIndex: 0,
            faceLayerBusy: false,
            regionOverlayActive: false,
            geometryEditActive: false,
            tool: "photo",
            cropDraft: null,
            cropAspect: "free",
            cropAngle: 0,
            previewFullFrame: false,
          });
        }
        set((state) => ({
          projectSelection: state.projectSelection.filter((id) => id !== projectId),
        }));
        await get().refreshRecent();
        toastSuccess("项目及本地资产已删除。", "项目已删除");
        return true;
      } catch (error) {
        errorToast(error, "没有删除项目，请重试。");
        return false;
      }
    },

    clearRenderCache: async () => {
      const api = get().api;
      if (!api) return null;
      try {
        return await api.clearRenderCache();
      } catch (error) {
        errorToast(error, "预览缓存没有清理完成，请重试。");
        return null;
      }
    },

    importBase64: async (fileName: string, dataBase64: string) => {
      const api = get().api;
      if (!api || get().importBusy) return;
      set({ importBusy: true });
      try {
        await settlePendingEdits();
        const next = await api.importPhotoBytes({
          fileName,
          dataBase64,
          collectionId: get().doc?.collectionId ?? null,
        });
        activateDocument(next);
        await get().refreshRecent();
      } catch (error) {
        errorToast(error, "这张照片无法打开，请换一张 JPG、PNG、TIFF、WebP 或 RAW 图片。");
      } finally {
        set({ importBusy: false });
      }
    },

    setTool: (tool) => {
      set((state) => {
        let selectedLayerId = state.selectedLayerId;
        if (state.doc) {
          if (tool === "portrait") {
            selectedLayerId = selectedOrKind(state.doc, state.selectedLayerId, "portrait")?.id ?? selectedLayerId;
          } else if (tool === "color") {
            selectedLayerId = selectedOrKind(state.doc, state.selectedLayerId, "color")?.id ?? selectedLayerId;
          }
        }
        let cropDraft: NormalizedCrop | null = null;
        let cropAngle = 0;
        if (tool === "crop") {
          const doc = state.doc;
          cropAngle = doc?.crop.angle ?? 0;
          if (state.cropDraft) {
            cropDraft = state.cropDraft;
          } else if (doc) {
            const frame = rotatedFrameSize(doc.canvas.width, doc.canvas.height, cropAngle);
            const uncropped =
              doc.crop.x === 0 &&
              doc.crop.y === 0 &&
              doc.crop.width >= frame.width &&
              doc.crop.height >= frame.height &&
              Math.abs(cropAngle) < 0.001;
            cropDraft = uncropped
              ? { ...FULL_CROP }
              : pixelsToRotatedDraft(doc.crop, frame.width, frame.height);
          } else {
            cropDraft = { ...FULL_CROP };
          }
          if (doc && cropDraft) {
            const bounds = cropBounds(doc.canvas.width, doc.canvas.height, cropAngle, null);
            cropDraft = clampCropToBounds(cropDraft, bounds, false);
          }
        }
        return {
          tool,
          selectedLayerId,
          cropDraft,
          cropAngle: tool === "crop" ? cropAngle : 0,
          previewFullFrame: tool === "crop",
          whiteBalancePickActive: false,
          ...(tool === "portrait" ? {} : { geometryEditActive: false }),
        };
      });
      schedulePreview(0);
    },

    setCropDraft: (draft) => {
      const { doc, cropAspect, cropAngle } = get();
      if (!doc) {
        set({ cropDraft: draft });
        return;
      }
      const ratio = aspectRatioOf(cropAspect);
      const bounds = cropBounds(doc.canvas.width, doc.canvas.height, cropAngle, ratio);
      set({ cropDraft: clampCropToBounds(draft, bounds, ratio !== null) });
    },

    setCropAngle: (angle) => {
      const nextAngle = Math.min(45, Math.max(-45, angle));
      const state = get();
      if (!state.doc || !state.cropDraft) {
        set({ cropAngle: nextAngle });
        return;
      }
      const ratio = aspectRatioOf(state.cropAspect);
      const cropDraft = fitCropForAngle(
        state.cropDraft,
        state.cropAngle,
        nextAngle,
        state.doc.canvas.width,
        state.doc.canvas.height,
        ratio,
      );
      set({ cropAngle: nextAngle, cropDraft });
    },

    setCropAspect: (aspect) => {
      const state = get();
      const doc = state.doc;
      if (!doc?.source) {
        set({ cropAspect: aspect });
        return;
      }
      const ratio = aspectRatioOf(aspect);
      if (ratio === null) {
        set({ cropAspect: aspect });
        return;
      }
      const bounds = cropBounds(doc.canvas.width, doc.canvas.height, state.cropAngle, ratio);
      set({ cropAspect: aspect, cropDraft: bounds });
    },

    applyCrop: async () => {
      const api = get().api;
      const doc = get().doc;
      const draft = get().cropDraft;
      if (!api || !doc || !draft) return;
      const angle = get().cropAngle;
      const frame = rotatedFrameSize(doc.canvas.width, doc.canvas.height, angle);
      const absolute = { ...draftToRotatedPixels(draft, frame.width, frame.height), angle };
      if (
        absolute.x === doc.crop.x &&
        absolute.y === doc.crop.y &&
        absolute.width === doc.crop.width &&
        absolute.height === doc.crop.height &&
        Math.abs((doc.crop.angle ?? 0) - angle) < 0.001
      ) {
        return;
      }
      await flushSync();
      try {
        const next = await api.setCrop({ projectId: doc.id, crop: absolute });
        applyDocument(next);
        set({
          cropDraft: pixelsToRotatedDraft(next.crop, frame.width, frame.height),
          cropAspect: "free",
        });
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "裁剪没有应用，请重试。");
      }
    },

    resetCrop: async () => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      if (
        doc.crop.x === 0 &&
        doc.crop.y === 0 &&
        doc.crop.width === doc.canvas.width &&
        doc.crop.height === doc.canvas.height &&
        Math.abs(doc.crop.angle ?? 0) < 0.001
      ) {
        set({ cropDraft: { ...FULL_CROP }, cropAngle: 0 });
        return;
      }
      await flushSync();
      try {
        const next = await api.setCrop({
          projectId: doc.id,
          crop: { x: 0, y: 0, width: doc.canvas.width, height: doc.canvas.height, angle: 0 },
        });
        applyDocument(next);
        set({ cropDraft: { ...FULL_CROP }, cropAspect: "free", cropAngle: 0 });
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "没有恢复完整照片，请重试。");
      }
    },

    cancelCrop: () => {
      set({ tool: "photo", cropDraft: null, cropAspect: "free", cropAngle: 0, previewFullFrame: false });
      schedulePreview(0);
    },

    selectLayer: (layerId) => set({ selectedLayerId: layerId }),
    setComparing: (comparing) => set({ comparing }),
    setSplitCompare: (enabled) => {
      if (enabled && Math.abs(get().doc?.crop.angle ?? 0) > 0.001) {
        toastError("旋转后的裁剪暂不支持分屏对比，可先清空角度。", "分屏对比不可用");
        return;
      }
      set({ splitCompare: enabled, comparing: false });
    },
    setRetouchMode: (retouchMode) => set({ retouchMode }),
    setRetouchStrength: (strength) =>
      set({ retouchStrength: Math.min(1, Math.max(0, strength)) }),
    setRetouchRadius: (radius) =>
      set({ retouchRadius: Math.min(0.2, Math.max(0.004, radius)) }),
    setRetouchSource: (retouchSource) => set({ retouchSource }),
    ensureRetouchLayer: async () => {
      const doc = get().doc;
      if (!doc) return null;
      const existing = doc.layers.find((layer) => layer.kind === "retouch");
      if (existing) {
        set({ selectedLayerId: existing.id });
        return existing;
      }
      return get().addLayer("retouch");
    },
    addRetouchSpot: async (spot) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      if (get().mode !== "tauri") {
        toastError("局部修复仅在桌面版可用。", "演示模式");
        return;
      }
      const layer = await get().ensureRetouchLayer();
      if (!layer) return;
      const latest = get().doc?.layers.find((item) => item.id === layer.id);
      const spots = [...(latest?.retouch?.spots ?? []), spot];
      const warp = latest?.retouch?.warp ?? [];
      const freeze = latest?.retouch?.freeze ?? [];
      const freezeGrid = latest?.retouch?.freezeGrid ?? [];
      try {
        const next = await api.updateLayerParams({
          projectId: doc.id,
          layerId: layer.id,
          retouch: { spots, warp, freeze, freezeGrid },
        });
        applyDocument(next);
        await commitLabel(spot.mode === "clone" ? "克隆图章" : "修复画笔");
        schedulePreview(40);
      } catch (error) {
        markSaveFailed();
        errorToast(error, "这笔修复没有保存，请重试。");
      }
    },
    addLiquifyStroke: async (stroke) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      if (get().mode !== "tauri") {
        toastError("液化仅在桌面版可用。", "演示模式");
        return;
      }
      if (stroke.points.length === 0) return;
      const layer = await get().ensureRetouchLayer();
      if (!layer) return;
      const latest = get().doc?.layers.find((item) => item.id === layer.id);
      const spots = latest?.retouch?.spots ?? [];
      const warp = [...(latest?.retouch?.warp ?? []), stroke];
      const freeze = latest?.retouch?.freeze ?? [];
      const freezeGrid = latest?.retouch?.freezeGrid ?? [];
      try {
        const next = await api.updateLayerParams({
          projectId: doc.id,
          layerId: layer.id,
          retouch: { spots, warp, freeze, freezeGrid },
        });
        applyDocument(next);
        await commitLabel("液化");
        schedulePreview(40);
      } catch (error) {
        markSaveFailed();
        errorToast(error, "这笔液化没有保存，请重试。");
      }
    },
    addFreezeStroke: async (stroke) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      if (get().mode !== "tauri") {
        toastError("液化保护仅在桌面版可用。", "演示模式");
        return;
      }
      if (stroke.points.length === 0) return;
      const layer = await get().ensureRetouchLayer();
      if (!layer) return;
      const latest = get().doc?.layers.find((item) => item.id === layer.id);
      const retouch = latest?.retouch;
      try {
        const next = await api.updateLayerParams({
          projectId: doc.id,
          layerId: layer.id,
          retouch: {
            spots: retouch?.spots ?? [],
            warp: retouch?.warp ?? [],
            freeze: [...(retouch?.freeze ?? []), stroke],
            freezeGrid: retouch?.freezeGrid ?? [],
          },
        });
        applyDocument(next);
        await commitLabel("液化保护");
        schedulePreview(40);
      } catch (error) {
        markSaveFailed();
        errorToast(error, "这笔保护没有保存，请重试。");
      }
    },
    clearFreezeStrokes: async () => {
      const api = get().api;
      const doc = get().doc;
      const layer = doc?.layers.find((item) => item.kind === "retouch") ?? null;
      if (!api || !doc || !layer?.retouch) return;
      const retouch = layer.retouch;
      try {
        const next = await api.updateLayerParams({
          projectId: doc.id,
          layerId: layer.id,
          retouch: {
            spots: retouch.spots,
            warp: retouch.warp,
            freeze: [],
            freezeGrid: [],
          },
        });
        applyDocument(next);
        await commitLabel("清除液化保护");
        schedulePreview(40);
      } catch (error) {
        markSaveFailed();
        errorToast(error, "清除保护没有保存，请重试。");
      }
    },
    toggleFreezeGridCell: async (index) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      if (get().mode !== "tauri") {
        toastError("网格保护仅在桌面版可用。", "演示模式");
        return;
      }
      if (index < 0 || index >= FREEZE_GRID_CELLS) return;
      const layer = await get().ensureRetouchLayer();
      if (!layer) return;
      const latest = get().doc?.layers.find((item) => item.id === layer.id);
      const retouch = latest?.retouch;
      const freezeGrid = [...(retouch?.freezeGrid ?? [])];
      while (freezeGrid.length < FREEZE_GRID_CELLS) freezeGrid.push(false);
      freezeGrid[index] = !freezeGrid[index];
      try {
        const next = await api.updateLayerParams({
          projectId: doc.id,
          layerId: layer.id,
          retouch: {
            spots: retouch?.spots ?? [],
            warp: retouch?.warp ?? [],
            freeze: retouch?.freeze ?? [],
            freezeGrid,
          },
        });
        applyDocument(next);
        await commitLabel("网格保护");
        schedulePreview(40);
      } catch (error) {
        markSaveFailed();
        errorToast(error, "网格保护没有保存，请重试。");
      }
    },

    setMaskBrushActive: (enabled) =>
      set({
        maskBrushActive: enabled,
        ...(enabled ? { splitCompare: false, comparing: false, whiteBalancePickActive: false } : {}),
      }),
    setMaskBrushRadius: (radius) => set({ maskBrushRadius: Math.min(0.25, Math.max(0.005, radius)) }),
    setMaskBrushErase: (maskBrushErase) => set({ maskBrushErase }),

    setWhiteBalancePickActive: (active) =>
      set({
        whiteBalancePickActive: active,
        ...(active
          ? { maskBrushActive: false, splitCompare: false, comparing: false }
          : { whiteBalancePicking: false }),
      }),

    pickWhiteBalance: async (point) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc || get().whiteBalancePicking) return;
      set({ whiteBalancePicking: true });
      try {
        const result = await api.pickWhiteBalance({
          projectId: doc.id,
          x: Math.min(1, Math.max(0, point.x)),
          y: Math.min(1, Math.max(0, point.y)),
        });
        if (!isCurrentProject(doc.id)) return;
        const neutral =
          Math.abs(result.temperature) < 0.05 && Math.abs(result.tint) < 0.05;
        let layer = selectedOrKind(get().doc, get().selectedLayerId, "color");
        if (!layer && neutral) {
          set({
            whiteBalancePickActive: false,
            lastWhiteBalanceSample: result.sample,
          });
          toastSuccess("这个采样点已经接近中性，没有新增调整。", "白平衡无需校正");
          return;
        }
        if (!layer) {
          layer = await get().ensureColorLayer();
          if (!layer) return;
        }
        if (!isCurrentProject(doc.id)) return;
        const latest = get().doc?.layers.find((item) => item.id === layer.id);
        if (!latest) return;
        get().editColor(layer.id, {
          ...(latest.color ?? DEFAULT_COLOR),
          temperature: result.temperature,
          tint: result.tint,
        });
        set({
          whiteBalancePickActive: false,
          lastWhiteBalanceSample: result.sample,
        });
        await finishEdit("白平衡吸管");
        const describe = (value: number): string =>
          `${value > 0 ? "+" : ""}${Math.round(value)}`;
        toastSuccess(
          `冷暖 ${describe(result.temperature)}，色调 ${describe(result.tint)}；已写入调色图层。`,
          "白平衡已校正",
        );
      } catch (error) {
        errorToast(error, "白平衡采样失败，请换一处中性灰再试。");
      } finally {
        set({ whiteBalancePicking: false });
      }
    },

    applyAutoTone: async () => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      try {
        const result = await api.autoTone({ projectId: doc.id });
        if (!isCurrentProject(doc.id)) return;
        const neutral =
          Math.abs(result.exposure) < 0.005 &&
          Math.abs(result.contrast) < 0.05 &&
          Math.abs(result.whites) < 0.05 &&
          Math.abs(result.blacks) < 0.05;
        let layer = selectedOrKind(get().doc, get().selectedLayerId, "color");
        if (!layer && neutral) {
          toastSuccess("画面曝光已经比较均衡，没有新增调整。", "无需自动校正");
          return;
        }
        if (!layer) {
          layer = await get().ensureColorLayer();
          if (!layer) return;
        }
        if (!isCurrentProject(doc.id)) return;
        const latest = get().doc?.layers.find((item) => item.id === layer.id);
        if (!latest) return;
        get().editColor(layer.id, {
          ...(latest.color ?? DEFAULT_COLOR),
          exposure: result.exposure,
          contrast: result.contrast,
          whites: result.whites,
          blacks: result.blacks,
        });
        await finishEdit("自动校正");
        const signed = (value: number, digits = 0): string =>
          `${value > 0 ? "+" : ""}${value.toFixed(digits)}`;
        toastSuccess(
          `曝光 ${signed(result.exposure, 2)}，对比度 ${signed(result.contrast)}；已写入调色图层。`,
          "自动校正完成",
        );
      } catch (error) {
        errorToast(error, "自动校正失败，请稍后重试。");
      }
    },

    importLut: async (file: File | null) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc || get().lutBusy) return;
      if (file && file.size > MAX_LUT_BYTES) {
        toastError("LUT 文件过大（最大 8MB），请换一个小一些的 .cube。");
        return;
      }
      set({ lutBusy: true });
      try {
        let imported: LutAssetResult | null = null;
        if (file) {
          imported = await api.importLut({
            projectId: doc.id,
            fileName: file.name,
            dataBase64: await blobToBase64(file),
          });
        } else if (get().mode === "tauri") {
          const path = await api.pickLutFile();
          if (path) imported = await api.importLut({ projectId: doc.id, path });
        } else {
          return;
        }
        if (!imported) return;
        if (!isCurrentProject(doc.id)) return;
        let layer = selectedOrKind(get().doc, get().selectedLayerId, "color");
        if (!layer) {
          layer = await get().ensureColorLayer();
          if (!layer) return;
        }
        if (!isCurrentProject(doc.id)) return;
        const latest = get().doc?.layers.find((item) => item.id === layer.id);
        get().editColor(layer.id, {
          ...(latest?.color ?? DEFAULT_COLOR),
          lutAssetId: imported.assetId,
          lutStrength: 1,
        });
        const asset = imported;
        set((state) => ({
          lutAssets: {
            ...state.lutAssets,
            [asset.assetId]: {
              name: asset.displayName,
              dimension: asset.dimension,
              size: asset.size,
            },
          },
        }));
        await finishEdit(`导入 LUT ${imported.displayName}`);
        toastSuccess(
          `${imported.displayName}（${imported.dimension} · ${imported.size}）已套用，拖动强度即可控制深浅。`,
          "LUT 已导入",
        );
      } catch (error) {
        errorToast(error, "LUT 导入失败，请确认这是一个有效的 .cube 文件。");
      } finally {
        set({ lutBusy: false });
      }
    },

    removeLut: async () => {
      const layer = selectedOrKind(get().doc, get().selectedLayerId, "color");
      if (!layer || !layer.color?.lutAssetId) return;
      get().editColor(layer.id, { ...layer.color, lutAssetId: null });
      await finishEdit("移除 LUT 风格");
    },

    detectFaces: async () => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc?.source || get().faceBusy) return;
      set({ faceBusy: true, faceError: null, faceAnalysisDocId: doc.id });
      try {
        const result = await api.analyzePortrait({ projectId: doc.id });
        if (!isCurrentProject(doc.id)) return;
        const selectedFaceIndex = bestFaceIndex(result.faces);
        const firstLayer =
          result.faces.length > 0
            ? portraitLayerForFace(get().doc, selectedFaceIndex)
            : null;
        set({
          faceAnalysis: result,
          ignoredFaceIds: [],
          selectedFaceIndex,
          ...(firstLayer ? { selectedLayerId: firstLayer.id } : {}),
        });
        if (result.faces.length > 0 && get().mode === "tauri") {
          // Fine-repair effects need eye/mouth geometry. Measure it in the
          // background so opening the panel does not require a second click.
          void get().analyzeFaceRegions({ silent: true });
        }
      } catch (error) {
        if (isCurrentProject(doc.id)) {
          set({ faceError: userMessageOf(error, "人脸检测没有完成，请稍后再试。") });
        }
      } finally {
        set({ faceBusy: false });
      }
    },

    setSelectedFaceIndex: (index) => {
      const state = get();
      const faces = state.faceAnalysis?.faces ?? [];
      if (faces.length === 0) {
        set({ selectedFaceIndex: 0 });
        return;
      }
      const next = Math.min(faces.length - 1, Math.max(0, index));
      const layer = portraitLayerForFace(state.doc, next);
      set({
        selectedFaceIndex: next,
        geometryEditActive: false,
        ...(layer
          ? { selectedLayerId: layer.id }
          : faces.length > 1
            ? { selectedLayerId: null }
            : {}),
      });
    },

    toggleFaceIgnored: (faceId) => {
      const state = get();
      const faces = state.faceAnalysis?.faces ?? [];
      const faceIndex = faces.findIndex((face) => face.faceId === faceId);
      if (faceIndex < 0) return;
      const ignored = new Set(state.ignoredFaceIds);
      const restoring = ignored.has(faceId);
      if (restoring) {
        ignored.delete(faceId);
      } else {
        if (faces.length - ignored.size <= 1) {
          toastError("至少保留一张人脸用于调整。", "无法忽略");
          return;
        }
        ignored.add(faceId);
      }
      set({ ignoredFaceIds: [...ignored] });
      if (restoring) {
        get().setSelectedFaceIndex(faceIndex);
        return;
      }
      if (faces[state.selectedFaceIndex]?.faceId === faceId) {
        let best = -1;
        faces.forEach((face, index) => {
          if (ignored.has(face.faceId)) return;
          if (best < 0 || face.confidence > faces[best].confidence) best = index;
        });
        if (best >= 0) get().setSelectedFaceIndex(best);
      }
    },

    ensureFacePortraitLayer: async (overrides) => {
      const state = get();
      const { api, doc, faceAnalysis, selectedFaceIndex, faceRegions } = state;
      if (!api || !doc) return null;
      const face = faceAnalysis?.faces[selectedFaceIndex] ?? null;
      if (!face) return selectedOrKind(doc, state.selectedLayerId, "portrait");
      const existing = portraitLayerForFace(doc, selectedFaceIndex);
      if (existing) return existing;
      const total = faceAnalysis?.faces.length ?? 0;
      if (total <= 1) {
        const legacy = selectedOrKind(doc, state.selectedLayerId, "portrait");
        if (legacy && legacy.portrait?.faceIndex == null && legacy.portrait?.bodyMode !== true) {
          return legacy;
        }
      }
      if (state.faceLayerBusy) return null;
      const measured = faceRegions.find((item) => item.faceId === face.faceId) ?? null;
      const params: PortraitParams = {
        ...DEFAULT_PORTRAIT,
        ...overrides,
        faceIndex: selectedFaceIndex,
        geometry: portraitGeometryForFace(
          face,
          doc.source?.width ?? doc.canvas.width,
          doc.source?.height ?? doc.canvas.height,
          measured,
        ),
      };
      const mask: LayerMask = {
        strokes: [],
        autoSkin: false,
        regions: [
          faceMaskRegion(
            face,
            doc.source?.width ?? doc.canvas.width,
            doc.source?.height ?? doc.canvas.height,
            measured,
          ),
        ],
        gradient: null,
        inverted: false,
        feather: 0,
        density: 1,
      };
      const previousIds = new Set(doc.layers.map((layer) => layer.id));
      set({ faceLayerBusy: true });
      try {
        const next = await api.addLayer({
          projectId: doc.id,
          kind: "portrait",
          name: faceLayerName(selectedFaceIndex, total),
          portrait: params,
          mask,
        });
        if (!isCurrentProject(doc.id)) return null;
        applyDocument(next);
        const created = next.layers.find((layer) => !previousIds.has(layer.id)) ?? null;
        if (created) set({ selectedLayerId: created.id });
        schedulePreview(60);
        return created;
      } catch (error) {
        errorToast(error, "没有成功创建这张脸的美颜层。");
        return null;
      } finally {
        set({ faceLayerBusy: false });
      }
    },

    ensureBodyPortraitLayer: async () => {
      const state = get();
      const { api, doc } = state;
      if (!api || !doc) return null;
      const existing =
        doc.layers.find((layer) => layer.kind === "portrait" && layer.portrait?.bodyMode) ??
        null;
      if (existing) return existing;
      if (state.faceLayerBusy) return null;
      const face = state.faceAnalysis?.faces[state.selectedFaceIndex] ?? null;
      const measured = face
        ? state.faceRegions.find((item) => item.faceId === face.faceId) ?? null
        : null;
      const geometry = face
        ? portraitGeometryForFace(
            face,
            doc.source?.width ?? doc.canvas.width,
            doc.source?.height ?? doc.canvas.height,
            measured,
          )
        : null;
      const params: PortraitParams = {
        ...DEFAULT_PORTRAIT,
        bodyMode: true,
        geometry,
      };
      const previousIds = new Set(doc.layers.map((layer) => layer.id));
      set({ faceLayerBusy: true });
      try {
        const next = await api.addLayer({
          projectId: doc.id,
          kind: "portrait",
          name: "身体美颜",
          portrait: params,
        });
        if (!isCurrentProject(doc.id)) return null;
        applyDocument(next);
        const created =
          next.layers.find(
            (layer) => !previousIds.has(layer.id) && layer.portrait?.bodyMode,
          ) ?? null;
        if (created) set({ selectedLayerId: created.id });
        schedulePreview(60);
        return created;
      } catch (error) {
        errorToast(error, "没有成功创建身体美颜层。");
        return null;
      } finally {
        set({ faceLayerBusy: false });
      }
    },

    analyzeFaceRegions: async (options) => {
      const silent = options?.silent === true;
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc || get().regionBusy) return;
      set({ regionBusy: true });
      try {
        const result = await api.analyzePortraitRegions({ projectId: doc.id });
        if (!isCurrentProject(doc.id)) return;
        set({
          faceRegions: result.faces,
          ...(silent ? {} : { regionOverlayActive: result.faces.length > 0 }),
        });
        // Upgrade fallback face-box geometry on layers that already use the
        // fine repairs, so toggling them before measurement still sharpens up.
        const faces = get().faceAnalysis?.faces ?? [];
        const width = get().doc?.source?.width ?? get().doc?.canvas.width ?? 1;
        const height = get().doc?.source?.height ?? get().doc?.canvas.height ?? 1;
        let upgraded = false;
        for (const layer of get().doc?.layers ?? []) {
          const params = layer.portrait;
          if (
            layer.kind !== "portrait" ||
            !params ||
            !(hasFineEffects(params) || params.bodyMode)
          ) {
            continue;
          }
          const stored = params.faceIndex;
          const faceIndex =
            stored != null && stored < faces.length
              ? stored
              : faces.length === 1
                ? 0
                : -1;
          if (faceIndex < 0) continue;
          const face = faces[faceIndex];
          const measured =
            result.faces.find((item) => item.faceId === face.faceId) ??
            result.faces[faceIndex] ??
            null;
          if (!measured) continue;
          const geometry = portraitGeometryForFace(face, width, height, measured);
          if (JSON.stringify(geometry) === JSON.stringify(params.geometry ?? null)) {
            continue;
          }
          get().editPortrait(layer.id, { ...params, geometry });
          upgraded = true;
        }
        if (upgraded) await finishEdit("更新精细修复五官");
        if (silent) return;
        if (result.faces.length === 0) {
          toastError(
            result.warnings[0]?.message ?? "没有在这张照片中定位到五官。",
            "区域识别未完成",
          );
        } else {
          const face = result.faces[0];
          toastSuccess(
            `定位到 ${face.regions.length} 个区域，可信度 ${Math.round(face.score * 100)}%。精细修复会直接使用这些区域。`,
            "五官区域已识别",
          );
        }
      } catch (error) {
        if (!silent) {
          errorToast(error, "区域识别失败，请确认照片里有一张正对镜头的人脸。");
        }
      } finally {
        set({ regionBusy: false });
      }
    },

    setRegionOverlayActive: (active) => set({ regionOverlayActive: active }),

    setGeometryEditActive: async (active) => {
      if (!active) {
        set({ geometryEditActive: false });
        return;
      }
      const state = get();
      if (!state.doc) return;
      if (state.mode !== "tauri") {
        toastError("手动校正五官需要本地人脸测量，仅在桌面版可用。", "仅桌面版支持");
        return;
      }
      const face = state.faceAnalysis?.faces[state.selectedFaceIndex] ?? null;
      if (!face) {
        toastError("先识别并选择一张人脸，再调整五官位置。", "没有可调整的人脸");
        return;
      }
      if (state.faceLayerBusy) return;
      const existing = portraitLayerForFace(state.doc, state.selectedFaceIndex);
      const layer = existing ?? (await state.ensureFacePortraitLayer());
      if (!layer) return;
      set({
        geometryEditActive: true,
        selectedLayerId: layer.id,
        regionOverlayActive: false,
        maskBrushActive: false,
        whiteBalancePickActive: false,
      });
      schedulePreview(0);
    },

    updateFaceGeometry: (geometry) => {
      const state = get();
      const doc = state.doc;
      if (!doc) return;
      const layer =
        portraitLayerForFace(doc, state.selectedFaceIndex) ??
        doc.layers.find(
          (item) =>
            item.id === state.selectedLayerId &&
            item.kind === "portrait" &&
            item.portrait?.faceIndex == null &&
            item.portrait?.bodyMode !== true,
        );
      if (!layer?.portrait) return;
      state.editPortrait(layer.id, { ...layer.portrait, geometry });
    },

    runAiDenoise: async (strengthOverride) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc || get().aiDenoiseBusy) return null;
      if (get().mode !== "tauri") {
        toastError("AI 降噪使用本地 ONNX 推理，仅在桌面版可用。", "仅桌面版支持");
        return null;
      }
      const { aiDenoiseModelPath } = useSettingsStore.getState();
      if (!aiDenoiseModelPath) {
        toastError("请先在设置中选择一个 .onnx 降噪模型。", "尚未配置模型");
        set({ settingsOpen: true });
        return null;
      }
      const strength =
        strengthOverride ?? useSettingsStore.getState().aiDenoiseStrength / 100;
      await flushSync();
      const previousIds = new Set(doc.layers.map((layer) => layer.id));
      set({ aiDenoiseBusy: true });
      try {
        const next = await api.runAiDenoise({
          projectId: doc.id,
          modelPath: aiDenoiseModelPath,
          strength,
        });
        if (!isCurrentProject(doc.id)) return null;
        applyDocument(next);
        const created =
          next.layers.find(
            (layer) => !previousIds.has(layer.id) && layer.kind === "aiResult",
          ) ?? null;
        if (created) set({ selectedLayerId: created.id });
        schedulePreview(30);
        toastSuccess(
          "本地模型已完成降噪，结果作为新图层加入；关闭图层即可对比原始细节。",
          "AI 降噪完成",
        );
        return created;
      } catch (error) {
        errorToast(error, "AI 降噪没有完成，请确认模型可用后重试。");
        return null;
      } finally {
        set({ aiDenoiseBusy: false });
      }
    },

    runPhotoHqDenoise: async (strengthOverride) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc || get().photoHqBusy) return null;
      if (get().mode !== "tauri") {
        toastError("Photo-HQ 使用本地 GPU 引擎，仅在桌面版可用。", "仅桌面版支持");
        return null;
      }
      const strength =
        strengthOverride ?? useSettingsStore.getState().photoHqStrength / 100;
      await flushSync();
      const previousIds = new Set(doc.layers.map((layer) => layer.id));
      const jobId =
        globalThis.crypto?.randomUUID?.() ??
        `photo-hq-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      set({ photoHqBusy: true, photoHqJobId: jobId });
      try {
        const next = await api.runPhotoHqDenoise({
          projectId: doc.id,
          strength,
          jobId,
        });
        if (!isCurrentProject(doc.id)) return null;
        applyDocument(next);
        const created =
          next.layers.find(
            (layer) => !previousIds.has(layer.id) && layer.kind === "aiResult",
          ) ?? null;
        if (created) set({ selectedLayerId: created.id });
        schedulePreview(30);
        toastSuccess(
          "Photo-HQ 已完成 4x 人像降噪，结果作为新图层加入；关闭图层即可对比原始细节。",
          "Photo-HQ 降噪完成",
        );
        return created;
      } catch (error) {
        if (isApiError(error) && error.code === "AI_DENOISE_CANCELLED") {
          toastInfo("已停止 Photo-HQ 降噪，没有生成新图层。", "已取消");
        } else {
          errorToast(error, "Photo-HQ 降噪没有完成，请确认显卡驱动支持 Vulkan 后重试。");
        }
        return null;
      } finally {
        set({ photoHqBusy: false, photoHqJobId: null });
      }
    },

    cancelPhotoHqDenoise: async () => {
      const api = get().api;
      const jobId = get().photoHqJobId;
      if (!api) return;
      if (!jobId) {
        toastInfo("没有正在运行的 Photo-HQ 降噪。", "无需取消");
        return;
      }
      try {
        const cancelled = await api.cancelAiDenoise({ jobId });
        toastInfo(
          cancelled ? "正在停止 Photo-HQ，请稍候…" : "任务已经结束，无需取消。",
          "取消请求已发送",
        );
      } catch (error) {
        errorToast(error, "没有成功取消 Photo-HQ 降噪，请稍后再试。");
      }
    },

    checkAiCapability: async () => {
      const api = get().api;
      const settings = useSettingsStore.getState();
      const key = `${settings.aiDenoiseEngine}|${settings.aiDenoiseModelPath ?? ""}`;
      const cached = get().aiCapability;
      if (cached && get().aiCapabilityKey === key) return cached;
      if (!api) {
        const unavailable = {
          photoHq: { available: false, reason: "应用尚未初始化" },
          onnx: { available: false, reason: "应用尚未初始化" },
        };
        return unavailable;
      }
      set({ aiCapabilityBusy: true });
      try {
        const capability = await api.checkAiDenoiseCapability({
          onnxModelPath: settings.aiDenoiseModelPath,
        });
        set({ aiCapability: capability, aiCapabilityKey: key });
        return capability;
      } catch (error) {
        const reason = userMessageOf(error, "AI 能力检测失败");
        const unavailable = {
          photoHq: { available: false, reason },
          onnx: { available: false, reason },
        };
        set({ aiCapability: unavailable, aiCapabilityKey: key });
        return unavailable;
      } finally {
        set({ aiCapabilityBusy: false });
      }
    },

    setAiDenoiseEnabled: async (enabled, strength) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) {
        return { ok: false, engine: null, reason: "没有打开的照片" };
      }
      const normalized = Math.min(100, Math.max(0, Math.round(strength)));
      const existing = latestAiDenoiseLayer(doc);

      if (!enabled) {
        if (existing?.visible) {
          await get().setLayerVisibility(existing.id, false);
        }
        let layer = selectedOrKind(get().doc, get().selectedLayerId, "color");
        if (!layer) layer = await get().ensureColorLayer();
        if (layer) {
          const latest = get().doc?.layers.find((item) => item.id === layer.id);
          get().editColor(layer.id, {
            ...(latest?.color ?? DEFAULT_COLOR),
            noiseLuma: normalized,
            noiseChroma: 0,
          });
          await finishEdit("改用普通降噪");
        }
        return { ok: true, engine: null, reason: null };
      }

      if (get().aiDenoiseBusy || get().photoHqBusy) {
        return { ok: false, engine: null, reason: "AI 降噪正在运行" };
      }
      const capability = await get().checkAiCapability();
      if (!isCurrentProject(doc.id)) {
        return {
          ok: false,
          engine: null,
          reason: "项目已切换",
          projectChanged: true,
        };
      }
      const settings = useSettingsStore.getState();
      const preferred =
        settings.aiDenoiseEngine === "onnx"
          ? (["onnx", "photoHq"] as const)
          : (["photoHq", "onnx"] as const);
      const engine = preferred.find((candidate) => capability[candidate].available) ?? null;
      if (!engine) {
        const reason =
          [capability.photoHq.reason, capability.onnx.reason]
            .filter((item): item is string => Boolean(item))
            .join("；") || "AI 降噪不可用";
        return { ok: false, engine: null, reason };
      }

      if (existing) {
        await get().setLayerVisibility(existing.id, true);
        if (!isCurrentProject(doc.id)) {
          return {
            ok: false,
            engine: null,
            reason: "项目已切换",
            projectChanged: true,
          };
        }
        get().setLayerOpacity(existing.id, normalized / 100);
        const layer = selectedOrKind(get().doc, get().selectedLayerId, "color");
        if (layer) {
          const latest = get().doc?.layers.find((item) => item.id === layer.id);
          get().editColor(layer.id, {
            ...(latest?.color ?? DEFAULT_COLOR),
            noiseLuma: 0,
            noiseChroma: 0,
          });
        }
        await finishEdit("开启 AI 降噪");
        set({ selectedLayerId: existing.id });
        return {
          ok: true,
          engine: existing.name.includes("Photo-HQ") ? "photoHq" : "onnx",
          reason: null,
        };
      }

      const created =
        engine === "photoHq"
          ? await get().runPhotoHqDenoise(1)
          : await get().runAiDenoise(1);
      if (!isCurrentProject(doc.id)) {
        return {
          ok: false,
          engine,
          reason: "项目已切换",
          projectChanged: true,
        };
      }
      if (!created) {
        return {
          ok: false,
          engine,
          reason:
            engine === "photoHq"
              ? "Photo-HQ 运行失败，已回退普通降噪"
              : "ONNX 降噪运行失败，已回退普通降噪",
        };
      }
      try {
        const next = await api.updateLayer({
          projectId: doc.id,
          layerId: created.id,
          patch: { opacity: normalized / 100 },
        });
        if (!isCurrentProject(doc.id)) {
          return {
            ok: false,
            engine,
            reason: "项目已切换",
            projectChanged: true,
          };
        }
        applyDocument(next);
        const color = topLayerOfKind(get().doc?.layers ?? [], "color");
        if (color) {
          get().editColor(color.id, {
            ...(color.color ?? DEFAULT_COLOR),
            noiseLuma: 0,
            noiseChroma: 0,
          });
        }
        await finishEdit("开启 AI 降噪");
        schedulePreview(30);
        set({ selectedLayerId: created.id });
      } catch (error) {
        errorToast(error, "AI 降噪强度没有保存，请重试。");
      }
      return { ok: true, engine, reason: null };
    },

    addMaskRegion: async (region) => {
      const doc = get().doc;
      if (!doc) return;
      let layer = selectedOrKind(doc, get().selectedLayerId, "portrait");
      if (!layer) layer = selectedOrKind(doc, get().selectedLayerId, "color");
      if (!layer) {
        layer = await get().ensureColorLayer();
        if (!layer) return;
      }
      const existing = normalizeMask(layer.mask);
      const regions = existing.regions ?? [];
      const duplicated = regions.some(
        (item) =>
          item.kind === region.kind &&
          Math.abs(item.centerX - region.centerX) < 0.002 &&
          Math.abs(item.centerY - region.centerY) < 0.002,
      );
      if (duplicated) {
        toastError("这个区域已经在修饰范围里了。", "区域已存在");
        return;
      }
      await persistLayerMask(layer.id, { ...existing, regions: [...regions, region] });
      await commitLabel(`加入${regionLabel(region.kind)}范围`);
    },

    removeMaskRegion: async (layerId, index) => {
      const layer = get().doc?.layers.find((item) => item.id === layerId);
      if (!layer) return;
      const existing = normalizeMask(layer.mask);
      const regions = (existing.regions ?? []).filter((_, position) => position !== index);
      await persistLayerMask(layer.id, { ...existing, regions });
      await commitLabel("移除修饰区域");
    },

    addMaskStroke: async (layerId, stroke) => {
      const api = get().api;
      const doc = get().doc;
      const layer = doc?.layers.find((item) => item.id === layerId);
      if (!api || !doc || !layer || (layer.kind !== "color" && layer.kind !== "portrait" && layer.kind !== "blur")) {
        return;
      }
      const previous = layer.mask;
      const strokes = [...(previous?.strokes ?? []), stroke].slice(-500);
      // A brush stroke must not discard the gradient, segmentation raster,
      // inversion or feather that are already on this mask.
      const mask: NonNullable<LayerState["mask"]> = {
        ...normalizeMask(previous),
        strokes,
      };
      patchLocalLayer(layerId, (item) => ({ ...item, mask }));
      await flushSync();
      try {
        const next = await api.setLayerMask({
          projectId: doc.id,
          layerId,
          mask,
        });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        patchLocalLayer(layerId, (item) => ({ ...item, mask: previous }));
        errorToast(error, "画笔修饰没有保存，请重试。");
      }
    },

    setAutoSkinMask: async (layerId, enabled) => {
      const api = get().api;
      const doc = get().doc;
      const layer = doc?.layers.find((item) => item.id === layerId);
      if (!api || !doc || !layer || (layer.kind !== "color" && layer.kind !== "portrait" && layer.kind !== "blur")) {
        return;
      }
      const previous = layer.mask;
      const base = normalizeMask(previous);
      const hasOtherContent =
        base.strokes.length > 0 ||
        (base.regions?.length ?? 0) > 0 ||
        base.gradient !== null ||
        base.raster !== null;
      // Only clear the mask when auto-skin really was its only content.
      const nextMask =
        enabled || hasOtherContent ? { ...base, autoSkin: enabled } : null;
      patchLocalLayer(layerId, (item) => ({ ...item, mask: nextMask }));
      await flushSync();
      try {
        const next = await api.setLayerMask({ projectId: doc.id, layerId, mask: nextMask });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        patchLocalLayer(layerId, (item) => ({ ...item, mask: previous }));
        errorToast(error, "自动肤色范围没有保存，请重试。");
      }
    },

    checkSegmentation: async () => {
      const api = get().api;
      if (!api) return;
      const path = useSettingsStore.getState().segmentationModelPath;
      const key = path ?? "";
      if (get().segmentationCapabilityKey === key && get().segmentationCapability) return;
      try {
        const capability = await api.checkSegmentationCapability({ modelPath: path });
        set({ segmentationCapability: capability, segmentationCapabilityKey: key });
      } catch {
        set({
          segmentationCapability: { available: false, reason: "无法读取分割模型状态。" },
          segmentationCapabilityKey: key,
        });
      }
    },

    applySegmentationMask: async (layerId) => {
      const api = get().api;
      const doc = get().doc;
      const path = useSettingsStore.getState().segmentationModelPath;
      if (!api || !doc) return;
      if (!path) {
        toastInfo("先到设置里选择分割模型，再生成智能蒙版。", "还没有分割模型");
        return;
      }
      if (get().segmentationBusy) return;
      set({ segmentationBusy: true });
      try {
        const next = await api.runSegmentationMask({
          projectId: doc.id,
          layerId,
          modelPath: path,
        });
        if (isCurrentProject(doc.id)) {
          applyDocument(next);
          schedulePreview(30);
          toastSuccess("已生成智能蒙版；可用画笔继续修整，或随时移除。");
        }
      } catch (error) {
        errorToast(error, "智能蒙版没有生成，请检查模型后重试。");
      } finally {
        set({ segmentationBusy: false });
      }
    },

    clearRasterMask: async (layerId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      try {
        const next = await api.clearRasterMask({ projectId: doc.id, layerId });
        if (isCurrentProject(doc.id)) {
          applyDocument(next);
          schedulePreview(30);
        }
      } catch (error) {
        errorToast(error, "移除智能蒙版失败，请重试。");
      }
    },

    setMaskGradient: async (layerId, gradient) => {
      const layer = get().doc?.layers.find((item) => item.id === layerId);
      if (!layer || (layer.kind !== "color" && layer.kind !== "portrait" && layer.kind !== "blur")) return;
      if (!layer.mask && !gradient) return;
      await persistLayerMask(layerId, {
        ...normalizeMask(layer.mask),
        gradient,
      });
    },

    setMaskOptions: async (layerId, options) => {
      const layer = get().doc?.layers.find((item) => item.id === layerId);
      if (!layer?.mask) return;
      const base = normalizeMask(layer.mask);
      const next: LayerMask = {
        ...base,
        ...options,
        feather:
          options.feather === undefined
            ? base.feather
            : Math.min(1, Math.max(0, options.feather)),
        density:
          options.density === undefined
            ? base.density
            : Math.min(1, Math.max(0, options.density)),
      };
      const unchanged =
        next.inverted === base.inverted &&
        Math.abs((next.feather ?? 0) - (base.feather ?? 0)) < 1e-4 &&
        Math.abs((next.density ?? 1) - (base.density ?? 1)) < 1e-4;
      if (unchanged) return;
      await persistLayerMask(layerId, next);
    },

    clearLayerMask: async (layerId) => {
      const api = get().api;
      const doc = get().doc;
      const layer = doc?.layers.find((item) => item.id === layerId);
      if (!api || !doc || !layer?.mask) return;
      const previous = layer.mask;
      patchLocalLayer(layerId, (item) => ({ ...item, mask: null }));
      await flushSync();
      try {
        const next = await api.setLayerMask({ projectId: doc.id, layerId, mask: null });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        patchLocalLayer(layerId, (item) => ({ ...item, mask: previous }));
        errorToast(error, "没有清除修饰范围，请重试。");
      }
    },
    setZoom: (zoom) => set({ zoom: Math.min(6, Math.max(0.5, zoom)) }),
    setFitPercent: (percent) => {
      if (Math.abs(get().fitPercent - percent) > 0.0005) set({ fitPercent: percent });
    },
    openExport: () => set({ exportOpen: true, settingsOpen: false, versionsOpen: false }),
    closeExport: () => set({ exportOpen: false }),
    openSettings: () => set({ settingsOpen: true, exportOpen: false, versionsOpen: false }),
    closeSettings: () => set({ settingsOpen: false }),
    refreshPreview: () => schedulePreview(0),

    toggleProjectSelection: (projectId) =>
      set((state) => ({
        projectSelection: state.projectSelection.includes(projectId)
          ? state.projectSelection.filter((id) => id !== projectId)
          : [...state.projectSelection, projectId],
      })),

    selectAllProjects: () =>
      set((state) => ({
        projectSelection: state.recent
          .filter(
            (project) =>
              project.collectionId === state.doc?.collectionId &&
              project.width > 0 &&
              project.height > 0,
          )
          .map((project) => project.id),
      })),

    clearProjectSelection: () => set({ projectSelection: [] }),

    exportSelectedProjects: async () => {
      const api = get().api;
      const doc = get().doc;
      const selectedIds = new Set(get().projectSelection);
      const projects = get().recent.filter(
        (project) =>
          selectedIds.has(project.id) &&
          project.width > 0 &&
          project.height > 0 &&
          (!doc || project.collectionId === doc.collectionId),
      );
      if (!api || projects.length === 0) return;
      if (get().mode !== "tauri") {
        toastError("批量导出需要选择本地文件夹，仅在桌面版可用。", "仅桌面版支持");
        return;
      }
      if (get().batchExport) return;
      const settings = useSettingsStore.getState();
      const watermark = await buildWatermarkOptions(settings);
      if (settings.watermarkEnabled && !watermark) {
        toastError(
          "导出水印已开启，但还没有水印文字或 Logo。",
          "水印内容为空",
        );
        return;
      }
      let folder: string | null = null;
      try {
        folder = await api.pickDirectory();
      } catch (error) {
        errorToast(error, "无法打开文件夹选择窗口，请重试。");
        return;
      }
      if (!folder) return;
      // Commit pending edits before exporting a list that may evict the current
      // session; otherwise the current project could lose uncommitted changes.
      if (!(await settlePendingEdits())) return;
      const format = settings.exportFormat;
      const folderPath = folder.replace(/[\\/]+$/, "");
      const usedStems = new Set<string>();
      set({
        batchExport: {
          total: projects.length,
          done: 0,
          currentName: projects[0]?.name ?? "",
          failed: 0,
        },
      });
      let done = 0;
      let failed = 0;
      let firstError: string | null = null;
      for (const project of projects) {
        set((state) => ({
          batchExport: state.batchExport
            ? { ...state.batchExport, currentName: project.name }
            : null,
        }));
        try {
          // Same template as the single-photo dialog; `index` keeps batches
          // sortable when several photos share a name.
          const base = renderExportStem(settings.exportNameTemplate, {
            name: project.name,
            collection: project.collectionName,
            width: project.width,
            height: project.height,
            index: done + 1,
          });
          let stem = base;
          let suffix = 2;
          while (usedStems.has(stem.toLowerCase())) {
            stem = `${base}-${suffix}`;
            suffix += 1;
          }
          usedStems.add(stem.toLowerCase());
          const jobId =
            globalThis.crypto?.randomUUID?.() ??
            `batch-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
          await api.exportImage({
            projectId: project.id,
            jobId,
            options: {
              path: `${folderPath}/${stem}.${extensionForFormat(format)}`,
              format,
              quality: settings.exportQuality,
              maxDimension: null,
              width: null,
              height: null,
              colorSpace: settings.exportColorSpace,
              metadata: settings.exportMetadata,
              bitDepth: format === "jpeg" ? "eight" : settings.exportBitDepth,
              watermark,
            },
          });
          done += 1;
        } catch (error) {
          failed += 1;
          firstError ??= userMessageOf(error, "这个项目导出失败。");
        }
        set((state) => ({
          batchExport: state.batchExport
            ? { ...state.batchExport, done, failed }
            : null,
        }));
      }
      set({ batchExport: null });
      if (failed === 0) {
        toastSuccess(
          `${done} 个项目已按当前导出设置保存到 ${folderPath}`,
          "批量导出完成",
        );
      } else {
        toastError(
          `${done} 个成功，${failed} 个失败${firstError ? `：${firstError}` : "。"}`,
          "批量导出未全部完成",
        );
      }
    },

    syncColorAdjustmentsToSelection: async () => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc || get().syncAdjustmentsBusy || get().batchExport) return;
      const selected = get().projectSelection.filter(
        (projectId) =>
          projectId !== doc.id &&
          get().recent.some(
            (project) =>
              project.id === projectId &&
              project.collectionId === doc.collectionId &&
              project.width > 0 &&
              project.height > 0,
          ),
      );
      if (selected.length === 0) {
        toastError("先在底部照片集勾选要应用调色的其他照片。", "没有可同步的照片");
        return;
      }
      if (!(await settlePendingEdits())) return;
      set({ syncAdjustmentsBusy: true });
      try {
        const result = await api.syncColorAdjustments({
          sourceProjectId: doc.id,
          projectIds: selected,
        });
        if (result.synced === 0) {
          toastError(
            result.failed.length > 0
              ? `${result.failed.length} 张照片未能同步。`
              : "没有可同步的照片。",
            "同步调色未完成",
          );
          return;
        }
        if (result.failed.length > 0) {
          const names = result.failed.slice(0, 3).join("、");
          toastError(
            `已同步 ${result.synced} 张，${result.failed.length} 张未能更新：${names}`,
            "同步调色部分完成",
          );
        } else {
          toastSuccess(
            `当前调色已同步到 ${result.synced} 张照片，可在各自项目中撤销。`,
            "同步调色完成",
          );
        }
        void get().refreshRecent();
      } catch (error) {
        errorToast(error, "同步调色失败，请重试。");
      } finally {
        set({ syncAdjustmentsBusy: false });
      }
    },

    undo: async () => {
      const api = get().api;
      await flushSync();
      const doc = get().doc;
      if (!api || !doc || !doc.canUndo) return;
      try {
        const next = await api.undo({ projectId: doc.id });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "无法撤销这一步，请重试。");
      }
    },

    redo: async () => {
      const api = get().api;
      await flushSync();
      const doc = get().doc;
      if (!api || !doc || !doc.canRedo) return;
      try {
        const next = await api.redo({ projectId: doc.id });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "无法重做这一步，请重试。");
      }
    },

    addLayer: async (kind, options) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return null;
      await flushSync();
      const previousIds = new Set(doc.layers.map((layer) => layer.id));
      try {
        const next = await api.addLayer({ projectId: doc.id, kind, ...options });
        applyDocument(next);
        const created =
          next.layers.find((layer) => !previousIds.has(layer.id) && layer.kind === kind) ??
          topLayerOfKind(next.layers, kind);
        if (created) set({ selectedLayerId: created.id });
        schedulePreview(40);
        return created;
      } catch (error) {
        errorToast(error, "没有成功添加调整图层，请重试。");
        return null;
      }
    },

    ensureColorLayer: async () => {
      const doc = get().doc;
      if (!doc) return null;
      const existing = selectedOrKind(doc, get().selectedLayerId, "color");
      if (existing) {
        set({ selectedLayerId: existing.id });
        return existing;
      }
      if (colorCreation) return colorCreation;
      colorCreation = get().addLayer("color");
      try {
        return await colorCreation;
      } finally {
        colorCreation = null;
      }
    },

    duplicateLayer: async (layerId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      const layer = doc.layers.find((item) => item.id === layerId);
      if (!layer || layer.kind === "base") return;
      await flushSync();
      const existing = new Set(doc.layers.map((item) => item.id));
      try {
        const next = await api.duplicateLayer({ projectId: doc.id, layerId });
        const copy = next.layers.find((item) => !existing.has(item.id));
        applyDocument(next);
        if (copy) set({ selectedLayerId: copy.id });
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "没有复制这个图层，请重试。");
      }
    },

    deleteLayer: async (layerId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      const layer = doc.layers.find((item) => item.id === layerId);
      if (!layer || layer.kind === "base" || layer.locked) return;
      await flushSync();
      try {
        const next = await api.deleteLayer({ projectId: doc.id, layerId });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "没有删除这个图层，请重试。");
      }
    },

    moveLayer: async (layerId, direction) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      await flushSync();
      try {
        const next = await api.moveLayer({ projectId: doc.id, layerId, direction });
        applyDocument(next);
        schedulePreview(40);
      } catch (error) {
        errorToast(error, "没有移动这个图层，请重试。");
      }
    },

    setLayerVisibility: async (layerId, visible) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      const layer = doc.layers.find((item) => item.id === layerId);
      if (!layer) return;
      patchLocalLayer(layerId, (item) => ({ ...item, visible }));
      schedulePreview(60);
      try {
        const next = await api.updateLayer({ projectId: doc.id, layerId, patch: { visible } });
        mergeServerLayer(next, layerId);
        schedulePreview(40);
      } catch (error) {
        patchLocalLayer(layerId, (item) => ({ ...item, visible: layer.visible }));
        errorToast(error, "没有更新图层显示状态，请重试。");
      }
    },

    setLayerBlendMode: async (layerId, blendMode) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      const layer = doc.layers.find((item) => item.id === layerId);
      if (!layer) return;
      patchLocalLayer(layerId, (item) => ({ ...item, blendMode }));
      schedulePreview(60);
      try {
        const next = await api.updateLayer({ projectId: doc.id, layerId, patch: { blendMode } });
        mergeServerLayer(next, layerId);
        schedulePreview(40);
      } catch (error) {
        patchLocalLayer(layerId, (item) => ({ ...item, blendMode: layer.blendMode }));
        errorToast(error, "混合方式没有更新，请重试。");
      }
    },

    setLayerOpacity: (layerId, opacity) => {
      patchLocalLayer(layerId, (item) => ({ ...item, opacity }));
      queueLayerPatch(layerId, { opacity });
      schedulePreview(120);
    },

    editPortrait: (layerId, params) => {
      patchLocalLayer(layerId, (item) => ({ ...item, portrait: { ...params } }));
      queueParams(layerId, { portrait: { ...params } });
      noteInteractiveEdit();
      schedulePreview(130);
    },

    copyLayerAdjustments: (layerId) => {
      const layer = get().doc?.layers.find((item) => item.id === layerId);
      if (layer?.color) {
        set({
          copiedAdjustments: { kind: "color", color: { ...layer.color } },
        });
        toastSuccess("已复制调色参数，可打开其他照片后粘贴。", "调色参数已复制");
      } else if (layer?.portrait) {
        set({
          copiedAdjustments: {
            kind: "portrait",
            portrait: { ...layer.portrait, faceIndex: null, geometry: null },
          },
        });
        toastSuccess("已复制人像参数，可打开其他照片后粘贴。", "人像参数已复制");
      }
    },

    pasteLayerAdjustments: async () => {
      const copied = get().copiedAdjustments;
      const api = get().api;
      const doc = get().doc;
      if (!copied || !api || !doc) return;
      if (copied.kind === "color") {
        const layer = await get().ensureColorLayer();
        if (!layer) return;
        set({ selectedLayerId: layer.id });
        get().editColor(layer.id, { ...copied.color });
        await finishEdit("粘贴调色参数");
        toastSuccess("调色参数已粘贴到当前照片，可继续微调。", "已粘贴调色参数");
        return;
      }
      let layer = selectedOrKind(get().doc, get().selectedLayerId, "portrait");
      if (!layer) layer = await get().addLayer("portrait");
      if (!layer) return;
      set({ selectedLayerId: layer.id });
      get().editPortrait(layer.id, {
        ...copied.portrait,
        bodyMode: layer.portrait?.bodyMode ?? false,
        faceIndex: layer.portrait?.faceIndex ?? null,
        geometry: layer.portrait?.geometry ?? geometryForLayer(layer),
      });
      await finishEdit("粘贴人像参数");
      toastSuccess("人像参数已粘贴到当前照片，可继续微调。", "已粘贴人像参数");
    },

    editColor: (layerId, params) => {
      patchLocalLayer(layerId, (item) => ({ ...item, color: { ...params } }));
      queueParams(layerId, { color: { ...params } });
      noteInteractiveEdit();
      schedulePreview(130);
    },

    editBlur: (layerId, params) => {
      patchLocalLayer(layerId, (item) => ({ ...item, blur: { ...params } }));
      queueParams(layerId, { blur: { ...params } });
      noteInteractiveEdit();
      schedulePreview(130);
    },

    commitEdit: async (label) => {
      await finishEdit(label);
    },

    applyLook: async (layerId, look) => {
      const doc = get().doc;
      const source = doc?.layers.find((item) => item.id === layerId);
      if (!doc) return;
      if (!source?.mask || !maskHasContent(source.mask)) {
        toastInfo(
          "先给这个图层加蒙版或智能蒙版，再套用一键效果。",
          "没有可用的蒙版",
        );
        return;
      }
      // "Background" is the inverse of a subject mask; the copy owns its own
      // inverted flag so the source layer stays untouched.
      const mask =
        look === "desaturateBackground"
          ? { ...source.mask, inverted: !(source.mask.inverted ?? false) }
          : { ...source.mask };
      const created = await get().addLayer("color", {
        name: look === "brightenSubject" ? "主体提亮" : "背景去色",
        mask,
      });
      if (!created) return;
      const params =
        look === "brightenSubject"
          ? { ...DEFAULT_COLOR, exposure: 0.35 }
          : { ...DEFAULT_COLOR, saturation: -100 };
      get().editColor(created.id, params);
      await get().commitEdit(
        look === "brightenSubject" ? "主体提亮" : "背景去色",
      );
      toastSuccess(
        look === "brightenSubject"
          ? "已新建「主体提亮」图层，可在它上面继续调曝光。"
          : "已新建「背景去色」图层（蒙版已反相），可继续调饱和度。",
        "已应用",
      );
    },

    flushPendingEdits: async () => settlePendingEdits(),

    retrySave: async () => {
      await settlePendingEdits();
    },

    commitPortrait: async (layerId, params, label) => {
      get().editPortrait(layerId, params);
      await finishEdit(label);
    },

    commitColor: async (layerId, params, label) => {
      get().editColor(layerId, params);
      await finishEdit(label);
    },

    applyPortraitPreset: async (presetId) => {
      const preset = PORTRAIT_PRESETS.find((item) => item.id === presetId);
      if (!preset) return;
      const state = get();
      const doc = state.doc;
      if (!doc) return;
      const intensity = useSettingsStore.getState().portraitIntensity;
      const face = state.faceAnalysis?.faces[state.selectedFaceIndex] ?? null;
      let layer = face ? portraitLayerForFace(doc, state.selectedFaceIndex) : null;
      if (face && !layer) {
        const total = state.faceAnalysis?.faces.length ?? 0;
        const legacy = selectedOrKind(doc, state.selectedLayerId, "portrait");
        if (total <= 1 && legacy && legacy.portrait?.faceIndex == null && legacy.portrait?.bodyMode !== true) {
          layer = legacy;
        } else {
          layer = await state.ensureFacePortraitLayer({ ...preset.settings, intensity });
          if (layer) set({ selectedLayerId: layer.id });
          return;
        }
      }
      if (!face) {
        const candidate = selectedOrKind(doc, state.selectedLayerId, "portrait");
        layer = candidate && !candidate.portrait?.bodyMode ? candidate : null;
        if (!layer) layer = await get().addLayer("portrait");
      }
      if (!layer) return;
      set({ selectedLayerId: layer.id });
      get().editPortrait(layer.id, {
        ...layer.portrait,
        ...preset.settings,
        intensity,
        faceIndex: layer.portrait?.faceIndex ?? null,
      });
      await finishEdit(preset.label);
    },

    applyColorPreset: async (presetId) => {
      const preset = COLOR_PRESETS.find((item) => item.id === presetId);
      if (!preset) return;
      const layer = await get().ensureColorLayer();
      if (!layer) return;
      set({ selectedLayerId: layer.id });
      get().editColor(layer.id, { ...preset.settings });
      await finishEdit(preset.label);
    },

    setPortraitEffect: async (layerId, key, enabled) => {
      let layer = get().doc?.layers.find((item) => item.id === layerId);
      if (!layer || !layer.portrait) return;
      const effect = PORTRAIT_EFFECTS.find((item) => item.key === key);
      if (!effect) return;
      const needsGeometry =
        effect.group === "fine" || effect.group === "beauty" || effect.group === "wrinkle";
      if (enabled && needsGeometry && !layer.portrait.geometry) {
        // Measure first when possible; the fallback face-box geometry still
        // keeps the effect usable if no regions can be found.
        if (get().faceRegions.length === 0) {
          await get().analyzeFaceRegions({ silent: true });
        }
        layer = get().doc?.layers.find((item) => item.id === layerId) ?? layer;
        if (!layer.portrait) return;
      }
      const geometry =
        layer.portrait.geometry ??
        (enabled && needsGeometry ? geometryForLayer(layer) : null);
      const next: PortraitParams = {
        ...layer.portrait,
        [key]: enabled ? effect.onValue : 0,
        ...(geometry ? { geometry } : {}),
      };
      get().editPortrait(layerId, next);
      await finishEdit(`${enabled ? "开启" : "关闭"}${effect.name}`);
    },

    resetPortrait: async (layerId) => {
      const layer = get().doc?.layers.find((item) => item.id === layerId);
      if (!layer || !layer.portrait) return;
      const bodyMode = layer.portrait.bodyMode;
      get().editPortrait(layerId, {
        ...DEFAULT_PORTRAIT,
        intensity: useSettingsStore.getState().portraitIntensity,
        bodyMode,
        faceIndex: bodyMode ? null : layer.portrait.faceIndex ?? null,
        geometry: bodyMode ? layer.portrait.geometry ?? null : null,
      });
      await finishEdit("恢复人像默认");
    },

    resetColor: async (layerId) => {
      const layer = get().doc?.layers.find((item) => item.id === layerId);
      if (!layer || !layer.color) return;
      const neutral: ColorParams = { ...DEFAULT_COLOR, curveX: [...DEFAULT_COLOR.curveX], curveY: [...DEFAULT_COLOR.curveY] };
      get().editColor(layerId, neutral);
      await finishEdit("复位调色");
    },

    loadJobs: async () => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      try {
        const jobs = await api.listAiJobs({ projectId: doc.id });
        if (get().doc?.id === doc.id) set({ jobs });
      } catch {
        // AI is optional; an empty list is a valid degraded state.
      }
    },

    saveProviderConfig: async (input, apiKey) => {
      const api = get().api;
      if (!api) return null;
      try {
        let provider = await api.saveProviderConfig(input);
        if (apiKey.trim().length > 0) {
          provider = await api.setProviderCredential({ providerId: provider.id, apiKey });
        }
        await get().refreshProviders();
        toastSuccess("AI 服务配置已保存，密钥只使用一次，不会在界面中显示。", "配置已保存");
        return provider;
      } catch (error) {
        errorToast(error, "AI 服务配置没有保存，请检查填写内容。");
        return null;
      }
    },

    testProvider: async (providerId) => {
      const api = get().api;
      if (!api) return null;
      try {
        return await api.testProvider({ providerId });
      } catch (error) {
        errorToast(error, "连接测试没有完成，请稍后再试。");
        return null;
      }
    },

    listProviderModels: async (providerId) => {
      const api = get().api;
      if (!api) return null;
      try {
        return await api.listProviderModels({ providerId });
      } catch (error) {
        errorToast(error, "无法获取模型列表，请检查服务地址和 API Key。");
        return null;
      }
    },

    submitAiJob: async (providerId, prompt, strength) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return null;
      try {
        const result = await api.submitAiJob({
          projectId: doc.id,
          providerId,
          operation: "edit",
          prompt,
          strength,
        });
        await get().loadJobs();
        return result.jobId;
      } catch (error) {
        errorToast(error, "AI 任务没有提交成功，请稍后再试。");
        return null;
      }
    },

    cancelAiJob: async (jobId) => {
      const api = get().api;
      if (!api) return;
      try {
        await api.cancelAiJob({ jobId });
        await get().loadJobs();
      } catch (error) {
        errorToast(error, "任务暂时无法取消，请稍后再试。");
      }
    },

    applyAiJob: async (jobId) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) return;
      await flushSync();
      const previousIds = new Set(doc.layers.map((layer) => layer.id));
      try {
        const next = await api.applyAiResult({ projectId: doc.id, jobId });
        if (!isCurrentProject(doc.id)) return;
        applyDocument(next);
        const created = next.layers.find((layer) => !previousIds.has(layer.id) && layer.kind === "aiResult");
        if (created) set({ selectedLayerId: created.id });
        schedulePreview(30);
        toastSuccess("AI 结果已作为新图层加入，可随时关闭或撤销。", "已应用 AI 结果");
      } catch (error) {
        errorToast(error, "AI 结果没有应用，请稍后再试。");
      }
    },

    receiveAiJob: (job) => {
      const doc = get().doc;
      if (!doc || doc.id !== job.projectId) return;
      set((state) => {
        const others = state.jobs.filter((item) => item.id !== job.id);
        return { jobs: [job, ...others] };
      });
    },

    chooseExportPath: async (defaultName, format) => {
      const api = get().api;
      if (!api) return null;
      return api.pickExportPath(defaultName, format);
    },

    exportPhoto: async (options) => {
      const api = get().api;
      const doc = get().doc;
      if (!api || !doc) throw new Error("NO_DOCUMENT");
      // Export renders the backend session, so pending slider deltas must reach
      // it before the file is produced.
      await flushSync();
      const jobId =
        globalThis.crypto?.randomUUID?.() ??
        `export-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      set({
        exportJob: {
          jobId,
          phase: "preparing",
          percent: 0,
          message: "准备导出…",
        },
      });
      try {
        return await api.exportImage({ projectId: doc.id, options, jobId });
      } finally {
        set({ exportJob: null });
      }
    },

    cancelExport: async () => {
      const job = get().exportJob;
      const api = get().api;
      if (!job || !api || job.phase === "cancelling") return;
      set({
        exportJob: { ...job, phase: "cancelling", message: "正在取消导出…" },
      });
      try {
        await api.cancelExport({ jobId: job.jobId });
      } catch {
        // The in-flight export will still finish or fail on its own.
      }
    },

    receiveExportProgress: (progress) => {
      if (get().exportJob?.jobId === progress.jobId) {
        set({ exportJob: progress });
      }
    },
  };
});
