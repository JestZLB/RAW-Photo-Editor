// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { DocumentState, LayerState } from "../api/types";
import { DEFAULT_BLUR } from "../lib/params";
import { useEditorStore } from "./editorStore";

const NOW = "2026-11-06T00:00:00.000Z";

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
    blur: null,
    mask: null,
    assetId: null,
    previewPath: null,
    createdAt: NOW,
    ...overrides,
  };
}

function blurDocument(): DocumentState {
  return {
    id: "project-a",
    name: "portrait",
    collectionId: "collection-one",
    collectionName: "人像",
    schemaVersion: 21,
    revision: 3,
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
      layer({ id: "layer-blur", name: "背景虚化", kind: "blur", blur: { ...DEFAULT_BLUR } }),
    ],
    canUndo: true,
    canRedo: false,
    history: ["添加背景虚化"],
  };
}

describe("blur layer", () => {
  let api: EditorApi;

  beforeEach(() => {
    api = {
      updateLayerParamsMeta: vi.fn(async () => ({ ...blurDocument(), layers: [] })),
      updateLayerParams: vi.fn(async () => blurDocument()),
      commitChanges: vi.fn(async () => blurDocument()),
      renderPreview: vi.fn(async () => ({
        path: "C:/cache/render.png",
        width: 1600,
        height: 800,
        revision: 4,
        durationMs: 1,
        cached: false,
        backend: "cpu",
        histogram: null,
      })),
    } as unknown as EditorApi;
    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: blurDocument(),
      selectedLayerId: "layer-blur",
      preview: null,
      autosave: "saved",
    });
  });

  it("patches the radius locally and sends it through the drag command", async () => {
    useEditorStore.getState().editBlur("layer-blur", { radius: 0.05 });

    // Local state updates immediately so the slider does not lag behind.
    const patched = useEditorStore
      .getState()
      .doc?.layers.find((item) => item.id === "layer-blur");
    expect(patched?.blur?.radius).toBe(0.05);

    // The queued write is debounced by 70 ms and merged per layer.
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(api.updateLayerParamsMeta).toHaveBeenCalledWith(
      expect.objectContaining({ layerId: "layer-blur", blur: { radius: 0.05 } }),
    );
    // The light-weight envelope must not be applied as a document.
    expect(
      useEditorStore.getState().doc?.layers.filter((item) => item.kind === "blur"),
    ).toHaveLength(1);
  });
});
