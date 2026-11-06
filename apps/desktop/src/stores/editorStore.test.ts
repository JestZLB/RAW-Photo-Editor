// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import { apiError } from "../api/errors";
import type {
  DocumentState,
  ExportImageRequest,
  ExportResult,
  ImportPhotoRequest,
  LayerState,
  ProjectSummary,
} from "../api/types";
import { useEditorStore } from "./editorStore";
import { useSettingsStore } from "./settingsStore";
import { useToastStore } from "./toastStore";

const projects: ProjectSummary[] = [
  {
    id: "project-a",
    name: "batch-a",
    collectionId: "collection-one",
    collectionName: "婚礼修图",
    width: 768,
    height: 966,
    updatedAt: "2026-10-23T00:00:00.000Z",
    thumbnailPath: null,
  },
  {
    id: "project-b",
    name: "batch-b",
    collectionId: "collection-one",
    collectionName: "婚礼修图",
    width: 768,
    height: 966,
    updatedAt: "2026-10-23T00:00:00.000Z",
    thumbnailPath: null,
  },
];

function exportResult(path: string): ExportResult {
  return {
    path,
    bytes: 1024,
    width: 768,
    height: 966,
    durationMs: 12,
    backend: "cpu",
    colorSpace: "sRGB",
    bitDepth: 8,
    metadataWritten: true,
    keepsLocation: false,
  };
}

describe("batch project export", () => {
  const calls: ExportImageRequest[] = [];
  let api: EditorApi;

  beforeEach(() => {
    vi.useFakeTimers();
    useToastStore.setState({ toasts: [] });
    calls.length = 0;
    api = {
      pickDirectory: vi.fn(async () => "C:\\exports"),
      exportImage: vi.fn(async (request: ExportImageRequest) => {
        calls.push(request);
        return exportResult(request.options.path);
      }),
    } as unknown as EditorApi;
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: null,
      recent: projects,
      projectSelection: projects.map((project) => project.id),
      batchExport: null,
      autosave: "saved",
    });
    useSettingsStore.setState({
      exportFormat: "jpeg",
      exportQuality: 88,
      exportColorSpace: "srgb",
      exportMetadata: "keep",
      exportBitDepth: "eight",
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    useEditorStore.setState({ batchExport: null, projectSelection: [] });
  });

  it("exports each selected project into the chosen folder", async () => {
    await useEditorStore.getState().exportSelectedProjects();

    expect(calls.map((call) => call.projectId)).toEqual(["project-a", "project-b"]);
    expect(calls.map((call) => call.options.path)).toEqual([
      "C:\\exports/batch-a-luma.jpg",
      "C:\\exports/batch-b-luma.jpg",
    ]);
    expect(calls[0].options.quality).toBe(88);
    expect(calls[0].options.bitDepth).toBe("eight");
    expect(useEditorStore.getState().batchExport).toBeNull();
    expect(useEditorStore.getState().projectSelection).toEqual(["project-a", "project-b"]);
  });

  it("does not start in the browser demo", async () => {
    useEditorStore.setState({ mode: "demo" });
    await useEditorStore.getState().exportSelectedProjects();
    expect(calls).toHaveLength(0);
    expect(api.pickDirectory).not.toHaveBeenCalled();
  });

  it("de-duplicates stems when project names collide", async () => {
    useEditorStore.setState({
      recent: [projects[0], { ...projects[1], name: "batch-a" }],
    });
    await useEditorStore.getState().exportSelectedProjects();
    expect(calls.map((call) => call.options.path)).toEqual([
      "C:\\exports/batch-a-luma.jpg",
      "C:\\exports/batch-a-luma-2.jpg",
    ]);
  });
});

describe("batch colour sync", () => {
  const currentDoc = {
    id: "project-a",
    collectionId: "collection-one",
    revision: 3,
    updatedAt: "2026-10-23T00:00:00.000Z",
    canUndo: false,
    canRedo: false,
    history: [],
    layers: [],
  } as unknown as import("../api/types").DocumentState;
  let api: EditorApi;
  let syncCalls: { sourceProjectId: string; projectIds: string[] }[];

  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    syncCalls = [];
    api = {
      commitChanges: vi.fn(async () => currentDoc),
      syncColorAdjustments: vi.fn(async (request) => {
        syncCalls.push(request);
        return { synced: request.projectIds.length, failed: [] };
      }),
      listProjects: vi.fn(async () => projects),
      listCollections: vi.fn(async () => []),
    } as unknown as EditorApi;
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: currentDoc,
      recent: [...projects, { ...projects[1], id: "project-c", collectionId: "other-set" }],
      projectSelection: ["project-a", "project-b", "project-c"],
      batchExport: null,
      syncAdjustmentsBusy: false,
      autosave: "saved",
    });
  });

  afterEach(() => {
    useEditorStore.setState({
      doc: null,
      projectSelection: [],
      syncAdjustmentsBusy: false,
    });
  });

  it("copies the current photo's colour stack to other photos in the set", async () => {
    await useEditorStore.getState().syncColorAdjustmentsToSelection();

    expect(syncCalls).toEqual([
      { sourceProjectId: "project-a", projectIds: ["project-b"] },
    ]);
    expect(useEditorStore.getState().syncAdjustmentsBusy).toBe(false);
  });

  it("does nothing when only the current photo is selected", async () => {
    useEditorStore.setState({ projectSelection: ["project-a"] });
    await useEditorStore.getState().syncColorAdjustmentsToSelection();
    expect(api.syncColorAdjustments).not.toHaveBeenCalled();
    expect(useEditorStore.getState().syncAdjustmentsBusy).toBe(false);
  });
});

function importDoc(id: string, collectionId: string, name: string): DocumentState {
  const now = "2026-10-23T00:00:00.000Z";
  return {
    id,
    name,
    collectionId,
    collectionName: "导入验证",
    schemaVersion: 16,
    revision: 1,
    createdAt: now,
    updatedAt: now,
    canvas: { width: 64, height: 48, workingColorSpace: "linear-srgb" },
    crop: { x: 0, y: 0, width: 1, height: 1 },
    source: null,
    layers: [
      {
        id: `base-${id}`,
        name: "原始照片",
        kind: "base",
        visible: true,
        locked: false,
        opacity: 1,
        blendMode: "normal",
        color: null,
        portrait: null,
        retouch: null,
        blur: null,
        mask: null,
        assetId: null,
        previewPath: null,
        createdAt: now,
      },
    ],
    canUndo: false,
    canRedo: false,
    history: [],
  };
}

describe("multi-photo import", () => {
  let calls: ImportPhotoRequest[];
  let api: EditorApi;

  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    calls = [];
    api = {
      pickImages: vi.fn(async () => [
        "C:/photos/first.png",
        "C:/photos/second.png",
        "C:/photos/third.png",
      ]),
      importPhoto: vi.fn(async (request: ImportPhotoRequest) => {
        calls.push(request);
        if (request.path.endsWith("second.png")) {
          throw new Error("decode failed");
        }
        return importDoc(
          request.path.endsWith("first.png") ? "new-first" : "new-third",
          "set-new",
          request.path,
        );
      }),
      commitChanges: vi.fn(async () => importDoc("project-a", "collection-one", "current")),
      listProjects: vi.fn(async () => projects),
      listCollections: vi.fn(async () => []),
    } as unknown as EditorApi;
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: null,
      recent: [],
      importBusy: false,
      importBatch: null,
    });
  });

  afterEach(() => {
    useEditorStore.setState({ doc: null, importBusy: false, importBatch: null });
  });

  it("creates a photo set from the first file and keeps the batch going", async () => {
    await useEditorStore.getState().openPhoto();

    expect(calls.map((call) => call.collectionId)).toEqual([
      null,
      "set-new",
      "set-new",
    ]);
    expect(useEditorStore.getState().doc?.id).toBe("new-first");
    expect(useEditorStore.getState().importBusy).toBe(false);
    expect(useEditorStore.getState().importBatch).toBeNull();
    const toasts = useToastStore.getState().toasts;
    expect(toasts.at(-1)?.kind).toBe("error");
    expect(toasts.at(-1)?.title).toBe("批量导入未全部完成");
    expect(toasts.at(-1)?.message).toContain("已导入 2 张");
  });

  it("adds every file to the open photo set without switching photos", async () => {
    const current = importDoc("project-a", "collection-one", "current");
    useEditorStore.setState({ doc: current });

    await useEditorStore.getState().openPhoto();

    expect(calls.map((call) => call.collectionId)).toEqual([
      "collection-one",
      "collection-one",
      "collection-one",
    ]);
    expect(useEditorStore.getState().doc?.id).toBe("project-a");
  });

  it("stays quiet when the picker is cancelled", async () => {
    (api.pickImages as ReturnType<typeof vi.fn>).mockResolvedValueOnce([]);
    await useEditorStore.getState().openPhoto();
    expect(api.importPhoto).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("imports all dropped paths through the same batch", async () => {
    await useEditorStore
      .getState()
      .importPaths(["C:/photos/first.png", "C:/photos/third.png"]);
    expect(calls.map((call) => call.collectionId)).toEqual([null, "set-new"]);
    expect(useEditorStore.getState().doc?.id).toBe("new-first");
  });
});

describe("manual keypoint correction", () => {
  const face = {
    faceId: "face-1",
    x: 10,
    y: 10,
    width: 100,
    height: 120,
    confidence: 0.9,
    models: ["centerface"],
  };
  const geometry = {
    face: { centerX: 0.5, centerY: 0.4, radiusX: 0.2, radiusY: 0.25, angle: 0, feather: 0.5 },
  };
  let api: EditorApi;
  let doc: DocumentState;

  beforeEach(() => {
    api = {
      updateLayerParams: vi.fn(async () => doc),
      commitChanges: vi.fn(async () => doc),
      listProjects: vi.fn(async () => projects),
      listCollections: vi.fn(async () => []),
    } as unknown as EditorApi;
    const base = importDoc("project-a", "collection-one", "current");
    doc = {
      ...base,
      layers: [
        base.layers[0],
        {
          id: "layer-face",
          name: "美颜 1",
          kind: "portrait",
          visible: true,
          locked: false,
          opacity: 1,
          blendMode: "normal",
          color: null,
          portrait: {
            intensity: 78,
            faceIndex: 0,
            bodyMode: false,
            geometry,
          } as unknown as import("../api/types").PortraitParams,
          retouch: null,
          blur: null,
          mask: null,
          assetId: null,
          previewPath: null,
          createdAt: "2026-10-23T00:00:00.000Z",
        } as LayerState,
      ],
    };
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc,
      tool: "portrait",
      faceAnalysis: { available: true, faces: [face], warnings: [] },
      faceRegions: [],
      selectedFaceIndex: 0,
      selectedLayerId: "layer-face",
      geometryEditActive: false,
      faceLayerBusy: false,
    });
  });

  afterEach(() => {
    useEditorStore.setState({ doc: null, geometryEditActive: false });
  });

  it("writes dragged ellipses to the selected face layer", () => {
    useEditorStore.getState().updateFaceGeometry({
      ...geometry,
      face: { ...geometry.face, centerX: 0.62, centerY: 0.5 },
    });
    const layer = useEditorStore
      .getState()
      .doc?.layers.find((item) => item.id === "layer-face");
    expect(layer?.portrait?.geometry?.face?.centerX).toBeCloseTo(0.62);
    expect(layer?.portrait?.geometry?.face?.centerY).toBeCloseTo(0.5);
  });

  it("enters on the face layer and leaves when another face is selected", async () => {
    await useEditorStore.getState().setGeometryEditActive(true);
    expect(useEditorStore.getState().geometryEditActive).toBe(true);
    expect(useEditorStore.getState().selectedLayerId).toBe("layer-face");

    useEditorStore.setState({
      faceAnalysis: {
        available: true,
        faces: [face, { ...face, faceId: "face-2" }],
        warnings: [],
      },
    });
    useEditorStore.getState().setSelectedFaceIndex(1);
    expect(useEditorStore.getState().geometryEditActive).toBe(false);
  });
});

describe("photo-hq cancellation", () => {
  it("cancels the running job and clears the busy state", async () => {
    let rejectRun: (error: unknown) => void = () => undefined;
    const run = new Promise<DocumentState>((_resolve, reject) => {
      rejectRun = reject;
    });
    const current = importDoc("project-a", "collection-one", "current");
    const api = {
      runPhotoHqDenoise: vi.fn(async () => run),
      cancelAiDenoise: vi.fn(async () => true),
      commitChanges: vi.fn(async () => current),
      updateLayerParams: vi.fn(async () => current),
      listProjects: vi.fn(async () => projects),
      listCollections: vi.fn(async () => []),
    } as unknown as EditorApi;
    useToastStore.setState({ toasts: [] });
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: current,
      photoHqBusy: false,
      photoHqJobId: null,
      autosave: "saved",
    });

    const pending = useEditorStore.getState().runPhotoHqDenoise(0.8);
    for (let attempt = 0; attempt < 20 && !useEditorStore.getState().photoHqJobId; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    const jobId = useEditorStore.getState().photoHqJobId;
    expect(jobId).toBeTruthy();
    expect(useEditorStore.getState().photoHqBusy).toBe(true);

    await useEditorStore.getState().cancelPhotoHqDenoise();
    expect(api.cancelAiDenoise).toHaveBeenCalledWith({ jobId });

    rejectRun(
      apiError(
        "AI_DENOISE_CANCELLED",
        "photo-hq run was cancelled",
        "已取消 Photo-HQ 降噪，没有生成新图层。",
      ),
    );
    await pending;
    expect(useEditorStore.getState().photoHqBusy).toBe(false);
    expect(useEditorStore.getState().photoHqJobId).toBeNull();
    expect(useToastStore.getState().toasts.at(-1)?.title).toBe("已取消");
  });
});


describe("empty photo set creation", () => {
  it("creates and opens a set without a photo", async () => {
    const empty = importDoc("empty-1", "empty-set", "空照片集");
    const api = {
      createProject: vi.fn(async () => empty),
      commitChanges: vi.fn(async () => empty),
      listProjects: vi.fn(async () => projects),
      listCollections: vi.fn(async () => []),
    } as unknown as EditorApi;
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: null,
      newProjectOpen: true,
      newProjectBusy: false,
    });
    const ok = await useEditorStore.getState().createEmptyProject("空照片集");
    expect(ok).toBe(true);
    expect(useEditorStore.getState().doc?.id).toBe("empty-1");
    expect(useEditorStore.getState().newProjectOpen).toBe(false);
    expect(api.createProject).toHaveBeenCalledWith({ name: "空照片集" });
  });
});
