// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { DocumentState, LayerState, VersionSummary } from "../api/types";
import { DEFAULT_COLOR } from "../lib/params";
import { useEditorStore } from "./editorStore";
import { useSettingsStore } from "./settingsStore";
import { useToastStore } from "./toastStore";

const NOW = "2026-11-05T00:00:00.000Z";

function layer(overrides: Partial<LayerState>): LayerState {
  return {
    id: "layer-base",
    name: "原始照片",
    kind: "base",
    visible: true,
    locked: false,
    opacity: 1,
    blendMode: "normal",
    color: null,
    portrait: null,
    retouch: null,
    mask: null,
    assetId: null,
    previewPath: null,
    createdAt: NOW,
    ...overrides,
  };
}

function documentWithExposure(exposure: number): DocumentState {
  return {
    id: "project-a",
    name: "portrait",
    collectionId: "collection-one",
    collectionName: "人像",
    schemaVersion: 20,
    revision: 4,
    createdAt: NOW,
    updatedAt: NOW,
    canvas: { width: 4000, height: 2000, workingColorSpace: "linear-srgb" },
    crop: { x: 0, y: 0, width: 4000, height: 2000, angle: 0 },
    source: {
      assetId: "asset-1",
      displayName: "portrait.jpg",
      width: 4000,
      height: 2000,
      format: "jpeg",
      originalPath: "C:/photos/portrait.jpg",
      previewPath: "C:/cache/preview.png",
      thumbnailPath: "C:/cache/thumb.png",
      colorSpace: "sRGB",
      hasSourceProfile: false,
    },
    layers: [
      layer({}),
      layer({
        id: "layer-color",
        name: "调色",
        kind: "color",
        color: { ...DEFAULT_COLOR, exposure },
      }),
    ],
    canUndo: true,
    canRedo: false,
    history: ["曝光"],
  };
}

function version(id: string, name: string): VersionSummary {
  return {
    id,
    name,
    revision: 4,
    createdAt: NOW,
    thumbnailPath: "C:/cache/preview.png",
  };
}

describe("version snapshots", () => {
  let versions: VersionSummary[];
  let api: EditorApi;

  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    useSettingsStore.setState({ previewMaxDimension: 1600 });
    versions = [version("version-1", "暖调")];
    api = {
      listVersions: vi.fn(async () => versions.map((item) => ({ ...item }))),
      createVersion: vi.fn(async (request: { name: string }) => {
        versions = [version("version-2", request.name.trim()), ...versions];
        return versions;
      }),
      deleteVersion: vi.fn(async (request: { versionId: string }) => {
        versions = versions.filter((item) => item.id !== request.versionId);
        return versions;
      }),
      restoreVersion: vi.fn(async () => documentWithExposure(0.5)),
      renderVersionPreview: vi.fn(async () => ({
        path: "C:/cache/version-1-d1600.png",
        width: 1200,
        height: 800,
        revision: 4,
        durationMs: 2,
        cached: false,
        backend: "cpu",
        histogram: null,
      })),
      renderPreview: vi.fn(async () => ({
        path: "C:/cache/render.png",
        width: 1600,
        height: 800,
        revision: 5,
        durationMs: 1,
        cached: false,
        backend: "cpu",
        histogram: null,
      })),
    } as unknown as EditorApi;
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: documentWithExposure(1.5),
      selectedLayerId: "layer-color",
      versions: [],
      versionsOpen: false,
      versionsBusy: false,
      autosave: "saved",
    });
  });

  it("loads the snapshot list when the dialog opens", async () => {
    await useEditorStore.getState().openVersions();
    expect(useEditorStore.getState().versionsOpen).toBe(true);
    expect(useEditorStore.getState().versions.map((item) => item.name)).toEqual(["暖调"]);
    expect(useEditorStore.getState().versionsBusy).toBe(false);
  });

  it("trims the name and ignores an empty one", async () => {
    await useEditorStore.getState().createVersion("   ");
    expect(api.createVersion).not.toHaveBeenCalled();

    await useEditorStore.getState().createVersion("  定稿  ");
    expect(api.createVersion).toHaveBeenCalledWith({ projectId: "project-a", name: "定稿" });
    expect(useEditorStore.getState().versions[0].name).toBe("定稿");
  });

  it("applies the restored document and keeps the layer selection", async () => {
    await useEditorStore.getState().restoreVersion("version-1");

    const doc = useEditorStore.getState().doc;
    expect(api.restoreVersion).toHaveBeenCalledWith({
      projectId: "project-a",
      versionId: "version-1",
    });
    expect(doc?.layers.find((item) => item.id === "layer-color")?.color?.exposure).toBe(0.5);
    // The layer still exists in the restored document, so the selection survives.
    expect(useEditorStore.getState().selectedLayerId).toBe("layer-color");
    expect(useEditorStore.getState().versionsBusy).toBe(false);
  });

  it("removes a snapshot from the list", async () => {
    await useEditorStore.getState().deleteVersion("version-1");
    expect(api.deleteVersion).toHaveBeenCalledWith({
      projectId: "project-a",
      versionId: "version-1",
    });
    expect(useEditorStore.getState().versions).toEqual([]);
  });

  it("renders a snapshot for side-by-side comparison", async () => {
    await useEditorStore.getState().openVersions();
    await useEditorStore.getState().compareVersion("version-1");

    expect(api.renderVersionPreview).toHaveBeenCalledWith({
      projectId: "project-a",
      versionId: "version-1",
      maxDimension: 1600,
    });
    const compare = useEditorStore.getState().versionCompare;
    expect(compare?.version.name).toBe("暖调");
    expect(compare?.path).toBe("C:/cache/version-1-d1600.png");
    expect(compare?.width).toBe(1200);

    useEditorStore.getState().clearVersionCompare();
    expect(useEditorStore.getState().versionCompare).toBeNull();
  });

  it("drops the comparison when its snapshot is deleted", async () => {
    await useEditorStore.getState().openVersions();
    await useEditorStore.getState().compareVersion("version-1");
    expect(useEditorStore.getState().versionCompare).not.toBeNull();

    await useEditorStore.getState().deleteVersion("version-1");
    expect(useEditorStore.getState().versionCompare).toBeNull();
  });
});
