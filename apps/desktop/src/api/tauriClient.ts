import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { EditorApi } from "./client";
import { normalizeApiError } from "./errors";
import type {
  AddLayerRequest,
  AiCapabilityRequest,
  AiDenoiseCapability,
  AiDenoiseRequest,
  AiJobInfo,
  AppInfo,
  ApplyAiResultRequest,
  AutoToneResult,
  CacheClearResult,
  CancelExportRequest,
  CollectionSummary,
  CommitChangesRequest,
  CreateProjectRequest,
  CreateVersionRequest,
  DeleteLayerRequest,
  DocumentState,
  DuplicateLayerRequest,
  ExportFormat,
  ExportImageRequest,
  ExportProgress,
  ExportResult,
  ImportLutRequest,
  ImportPhotoBytesRequest,
  ImportPhotoRequest,
  JobIdRequest,
  LutAssetResult,
  MoveLayerRequest,
  OpenProjectRequest,
  PhotoHqDenoiseRequest,
  PickWhiteBalanceRequest,
  PortraitAnalysisResult,
  PortraitRegionsResult,
  PreviewResult,
  ProjectRef,
  ProjectSummary,
  ProviderConfigInput,
  ProviderIdRequest,
  ProviderInfo,
  ProviderTestResult,
  RenameProjectRequest,
  RenderPreviewRequest,
  SetCropRequest,
  SetLayerMaskRequest,
  SetProviderCredentialRequest,
  SubmitAiJobRequest,
  SyncColorAdjustmentsRequest,
  SyncColorAdjustmentsResult,
  UpdateLayerParamsRequest,
  UpdateLayerRequest,
  VersionIdRequest,
  VersionSummary,
  WhiteBalanceResult,
} from "./types";

const IMAGE_EXTENSIONS = [
  "jpg",
  "jpeg",
  "png",
  "tif",
  "tiff",
  "webp",
  "cr2",
  "cr3",
  "nef",
  "nrw",
  "arw",
  "dng",
  "raf",
  "rw2",
  "rwl",
  "orf",
  "pef",
  "srw",
  "raw",
];

async function call<T>(command: string, req?: unknown): Promise<T> {
  try {
    if (req === undefined) return await invoke<T>(command);
    return await invoke<T>(command, { req });
  } catch (error) {
    throw normalizeApiError(error);
  }
}

export function createTauriClient(): EditorApi {
  return {
    mode: "tauri",

    getAppInfo: () => call<AppInfo>("get_app_info"),
    listProjects: () => call<ProjectSummary[]>("list_projects"),
    listCollections: () => call<CollectionSummary[]>("list_collections"),
    createProject: (req: CreateProjectRequest) => call<DocumentState>("create_project", req),
    openProject: (req: OpenProjectRequest) => call<DocumentState>("open_project", req),
    listVersions: (req: ProjectRef) => call<VersionSummary[]>("list_versions", req),
    createVersion: (req: CreateVersionRequest) => call<VersionSummary[]>("create_version", req),
    restoreVersion: (req: VersionIdRequest) =>
      call<DocumentState>("restore_version", req),
    deleteVersion: (req: VersionIdRequest) => call<VersionSummary[]>("delete_version", req),
    renameProject: (req: RenameProjectRequest) =>
      call<ProjectSummary>("rename_project", req),
    renameCollection: (req) => call<void>("rename_collection", req),
    syncColorAdjustments: (req: SyncColorAdjustmentsRequest) =>
      call<SyncColorAdjustmentsResult>("sync_color_adjustments", req),
    deleteProject: (req: ProjectRef) => call<void>("delete_project", req),
    clearRenderCache: () => call<CacheClearResult>("clear_render_cache"),
    importPhoto: (req: ImportPhotoRequest) => call<DocumentState>("import_photo", req),
    importPhotoBytes: (req: ImportPhotoBytesRequest) =>
      call<DocumentState>("import_photo_bytes", req),
    importLut: (req: ImportLutRequest) => call<LutAssetResult>("import_lut", req),

    renderPreview: (req: RenderPreviewRequest) => call<PreviewResult>("render_preview", req),
    updateLayerParams: (req: UpdateLayerParamsRequest) =>
      call<DocumentState>("update_layer_params", req),
    updateLayerParamsMeta: (req: UpdateLayerParamsRequest) =>
      call<DocumentState>("update_layer_params_meta", req),
    commitChanges: (req: CommitChangesRequest) => call<DocumentState>("commit_changes", req),
    setLayerMask: (req: SetLayerMaskRequest) => call<DocumentState>("set_layer_mask", req),
    autoTone: (req: ProjectRef) => call<AutoToneResult>("auto_tone", req),
    pickWhiteBalance: (req: PickWhiteBalanceRequest) =>
      call<WhiteBalanceResult>("pick_white_balance", req),
    setCrop: (req: SetCropRequest) => call<DocumentState>("set_crop", req),
    addLayer: (req: AddLayerRequest) => call<DocumentState>("add_layer", req),
    updateLayer: (req: UpdateLayerRequest) => call<DocumentState>("update_layer", req),
    duplicateLayer: (req: DuplicateLayerRequest) =>
      call<DocumentState>("duplicate_layer", req),
    deleteLayer: (req: DeleteLayerRequest) => call<DocumentState>("delete_layer", req),
    moveLayer: (req: MoveLayerRequest) => call<DocumentState>("move_layer", req),
    undo: (req: ProjectRef) => call<DocumentState>("undo", req),
    redo: (req: ProjectRef) => call<DocumentState>("redo", req),
    exportImage: (req: ExportImageRequest) => call<ExportResult>("export_image", req),
    cancelExport: (req: CancelExportRequest) => call<boolean>("cancel_export", req),
    subscribeExportProgress: async (listener) => {
      const unlisten = await listen<ExportProgress>("export-progress", (event) => {
        listener(event.payload);
      });
      return unlisten;
    },
    analyzePortrait: (req: ProjectRef) => call<PortraitAnalysisResult>("analyze_portrait", req),
    analyzePortraitRegions: (req: ProjectRef) =>
      call<PortraitRegionsResult>("analyze_portrait_regions", req),
    runAiDenoise: (req: AiDenoiseRequest) => call<DocumentState>("run_ai_denoise", req),
    runPhotoHqDenoise: (req: PhotoHqDenoiseRequest) =>
      call<DocumentState>("run_photo_hq_denoise", req),
    cancelAiDenoise: (req: JobIdRequest) => call<boolean>("cancel_ai_denoise", req),
    checkAiDenoiseCapability: (req: AiCapabilityRequest) =>
      call<AiDenoiseCapability>("check_ai_denoise_capability", req),

    listProviders: () => call<ProviderInfo[]>("list_providers"),
    saveProviderConfig: (req: ProviderConfigInput) =>
      call<ProviderInfo>("save_provider_config", req),
    setProviderCredential: (req: SetProviderCredentialRequest) =>
      call<ProviderInfo>("set_provider_credential", req),
    testProvider: (req: ProviderIdRequest) => call<ProviderTestResult>("test_provider", req),
    listProviderModels: (req: ProviderIdRequest) =>
      call<string[]>("list_provider_models", req),
    submitAiJob: (req: SubmitAiJobRequest) => call<{ jobId: string }>("submit_ai_job", req),
    cancelAiJob: (req: JobIdRequest) => call<void>("cancel_ai_job", req),
    listAiJobs: (req: ProjectRef) => call<AiJobInfo[]>("list_ai_jobs", req),
    applyAiResult: (req: ApplyAiResultRequest) => call<DocumentState>("apply_ai_result", req),
    subscribeAiJobs: async (listener) => {
      const unlisten = await listen<AiJobInfo>("ai-job-updated", (event) => {
        listener(event.payload);
      });
      return unlisten;
    },

    pickImage: async () => {
      try {
        const selected = await open({
          title: "打开照片",
          multiple: false,
          directory: false,
          filters: [{ name: "照片与 RAW", extensions: [...IMAGE_EXTENSIONS] }],
        });
        if (Array.isArray(selected)) return selected[0] ?? null;
        return selected;
      } catch (error) {
        throw normalizeApiError(error, "无法打开文件选择窗口，请重试。");
      }
    },

    pickImages: async () => {
      try {
        const selected = await open({
          title: "打开照片（可多选）",
          multiple: true,
          directory: false,
          filters: [{ name: "照片与 RAW", extensions: [...IMAGE_EXTENSIONS] }],
        });
        if (Array.isArray(selected)) return selected;
        return selected ? [selected] : [];
      } catch (error) {
        throw normalizeApiError(error, "无法打开文件选择窗口，请重试。");
      }
    },

    pickLutFile: async () => {
      try {
        const selected = await open({
          title: "导入 LUT",
          multiple: false,
          directory: false,
          filters: [{ name: "Cube LUT", extensions: ["cube"] }],
        });
        if (Array.isArray(selected)) return selected[0] ?? null;
        return selected;
      } catch (error) {
        throw normalizeApiError(error, "无法打开文件选择窗口，请重试。");
      }
    },

    pickDenoiseModel: async () => {
      try {
        const selected = await open({
          title: "选择 AI 降噪模型",
          multiple: false,
          directory: false,
          filters: [{ name: "ONNX 模型", extensions: ["onnx"] }],
        });
        if (Array.isArray(selected)) return selected[0] ?? null;
        return selected;
      } catch (error) {
        throw normalizeApiError(error, "无法打开文件选择窗口，请重试。");
      }
    },

    pickExportPath: async (defaultName: string, format: ExportFormat) => {
      const extensions = format === "jpeg" ? ["jpg"] : [format];
      try {
        const selected = await save({
          title: "导出照片",
          defaultPath: defaultName,
          filters: [{ name: format.toUpperCase(), extensions }],
        });
        return selected;
      } catch (error) {
        throw normalizeApiError(error, "无法打开保存窗口，请重试。");
      }
    },

    pickDirectory: async () => {
      try {
        const selected = await open({
          title: "选择批量导出文件夹",
          multiple: false,
          directory: true,
        });
        if (Array.isArray(selected)) return selected[0] ?? null;
        return selected;
      } catch (error) {
        throw normalizeApiError(error, "无法打开文件夹选择窗口，请重试。");
      }
    },
  };
}
