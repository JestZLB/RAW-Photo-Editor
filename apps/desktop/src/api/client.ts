import { isTauriRuntime } from "./assets";
import { DemoBackend } from "./demoBackend";
import { createTauriClient } from "./tauriClient";
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
  DeleteLayerRequest,
  DocumentState,
  DuplicateLayerRequest,
  ExportFormat,
  ExportImageRequest,
  ExportProgress,
  ExportResult,
  ImportPhotoBytesRequest,
  ImportPhotoRequest,
  ImportLutRequest,
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
  WhiteBalanceResult,
} from "./types";

export type EditorMode = "tauri" | "demo";

export interface EditorApi {
  readonly mode: EditorMode;

  getAppInfo(): Promise<AppInfo>;
  listProjects(): Promise<ProjectSummary[]>;
  listCollections(): Promise<CollectionSummary[]>;
  createProject(req: CreateProjectRequest): Promise<DocumentState>;
  openProject(req: OpenProjectRequest): Promise<DocumentState>;
  renameProject(req: RenameProjectRequest): Promise<ProjectSummary>;
  /** Rename every photo's photo set in one step (metadata only). */
  renameCollection(req: { collectionId: string; name: string }): Promise<void>;
  /** Copy this photo's colour layers onto other photos of the same set. */
  syncColorAdjustments(req: SyncColorAdjustmentsRequest): Promise<SyncColorAdjustmentsResult>;
  deleteProject(req: ProjectRef): Promise<void>;
  clearRenderCache(): Promise<CacheClearResult>;
  importPhoto(req: ImportPhotoRequest): Promise<DocumentState>;
  importPhotoBytes(req: ImportPhotoBytesRequest): Promise<DocumentState>;
  importLut(req: ImportLutRequest): Promise<LutAssetResult>;

  renderPreview(req: RenderPreviewRequest): Promise<PreviewResult>;
  updateLayerParams(req: UpdateLayerParamsRequest): Promise<DocumentState>;
  /**
   * Same write as `updateLayerParams`, but the response carries an empty
   * `layers` array. Only revision / history are usable — do not apply it as a
   * document. Intended for interactive drags that would otherwise move every
   * mask and retouch point over IPC on each tick.
   */
  updateLayerParamsMeta(req: UpdateLayerParamsRequest): Promise<DocumentState>;
  commitChanges(req: CommitChangesRequest): Promise<DocumentState>;
  setLayerMask(req: SetLayerMaskRequest): Promise<DocumentState>;
  autoTone(req: ProjectRef): Promise<AutoToneResult>;
  pickWhiteBalance(req: PickWhiteBalanceRequest): Promise<WhiteBalanceResult>;
  setCrop(req: SetCropRequest): Promise<DocumentState>;
  addLayer(req: AddLayerRequest): Promise<DocumentState>;
  updateLayer(req: UpdateLayerRequest): Promise<DocumentState>;
  duplicateLayer(req: DuplicateLayerRequest): Promise<DocumentState>;
  deleteLayer(req: DeleteLayerRequest): Promise<DocumentState>;
  moveLayer(req: MoveLayerRequest): Promise<DocumentState>;
  undo(req: ProjectRef): Promise<DocumentState>;
  redo(req: ProjectRef): Promise<DocumentState>;
  exportImage(req: ExportImageRequest): Promise<ExportResult>;
  cancelExport(req: CancelExportRequest): Promise<boolean>;
  subscribeExportProgress(listener: (progress: ExportProgress) => void): Promise<() => void>;
  analyzePortrait(req: ProjectRef): Promise<PortraitAnalysisResult>;
  analyzePortraitRegions(req: ProjectRef): Promise<PortraitRegionsResult>;
  runAiDenoise(req: AiDenoiseRequest): Promise<DocumentState>;
  runPhotoHqDenoise(req: PhotoHqDenoiseRequest): Promise<DocumentState>;
  /** Ask the running Photo-HQ engine to stop; resolves whether a task existed. */
  cancelAiDenoise(req: JobIdRequest): Promise<boolean>;
  checkAiDenoiseCapability(req: AiCapabilityRequest): Promise<AiDenoiseCapability>;

  listProviders(): Promise<ProviderInfo[]>;
  saveProviderConfig(req: ProviderConfigInput): Promise<ProviderInfo>;
  setProviderCredential(req: SetProviderCredentialRequest): Promise<ProviderInfo>;
  testProvider(req: ProviderIdRequest): Promise<ProviderTestResult>;
  listProviderModels(req: ProviderIdRequest): Promise<string[]>;
  submitAiJob(req: SubmitAiJobRequest): Promise<{ jobId: string }>;
  cancelAiJob(req: JobIdRequest): Promise<void>;
  listAiJobs(req: ProjectRef): Promise<AiJobInfo[]>;
  applyAiResult(req: ApplyAiResultRequest): Promise<DocumentState>;
  subscribeAiJobs(listener: (job: AiJobInfo) => void): Promise<() => void>;

  /* Desktop-only affordances. The demo implementation resolves without a dialog. */
  pickImage(): Promise<string | null>;
  /** Multi-select picker; resolves an empty list when the dialog is cancelled. */
  pickImages(): Promise<string[]>;
  pickLutFile(): Promise<string | null>;
  pickDenoiseModel(): Promise<string | null>;
  pickExportPath(defaultName: string, format: ExportFormat): Promise<string | null>;
  /** Folder picker for batch export (desktop only). */
  pickDirectory(): Promise<string | null>;
}

let cachedApi: EditorApi | null = null;

export function getApi(): EditorApi {
  if (cachedApi) return cachedApi;
  cachedApi = isTauriRuntime() ? createTauriClient() : new DemoBackend();
  return cachedApi;
}
