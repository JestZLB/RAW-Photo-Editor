// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { DocumentState, LayerMask, LayerState, SetLayerMaskRequest } from "../api/types";
import { useEditorStore } from "./editorStore";

const NOW = "2026-11-06T00:00:00.000Z";
const RASTER = {
  assetId: "0f8fad5b-d9cb-469f-a165-70867728950e",
  threshold: 0.55,
  softness: 0.2,
  source: "onnx:foreground",
};

const FULL_MASK: LayerMask = {
  strokes: [],
  autoSkin: false,
  regions: [],
  gradient: { kind: "linear", start: { x: 0, y: 0 }, end: { x: 1, y: 1 } },
  inverted: true,
  feather: 0.3,
  density: 0.75,
  raster: RASTER,
};

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

function documentWithBlurLayer(mask: LayerMask): DocumentState {
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
      layer({
        id: "layer-blur",
        name: "背景虚化",
        kind: "blur",
        blur: { radius: 0.03 },
        mask,
      }),
    ],
    canUndo: true,
    canRedo: false,
    history: ["添加背景虚化"],
  };
}

describe("mask edits on a blur layer", () => {
  let api: EditorApi;
  let doc: DocumentState;

  beforeEach(() => {
    doc = documentWithBlurLayer(FULL_MASK);
    api = {
      setLayerMask: vi.fn(async (req: SetLayerMaskRequest) => ({
        ...doc,
        layers: doc.layers.map((item) =>
          item.id === req.layerId ? { ...item, mask: req.mask } : item,
        ),
      })),
      updateLayerParamsMeta: vi.fn(async () => ({ ...doc, layers: [] })),
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
      doc,
      selectedLayerId: "layer-blur",
      preview: null,
      autosave: "saved",
    });
  });

  function lastSentMask(): LayerMask | null {
    const calls = vi.mocked(api.setLayerMask).mock.calls;
    return calls.length ? (calls[calls.length - 1][0].mask ?? null) : null;
  }

  it("accepts a brush stroke and keeps every other mask field", async () => {
    await useEditorStore.getState().addMaskStroke("layer-blur", {
      points: [{ x: 0.2, y: 0.2 }],
      radius: 0.1,
      feather: 0.2,
      erase: false,
    });

    expect(api.setLayerMask).toHaveBeenCalledTimes(1);
    const sent = lastSentMask();
    expect(sent?.strokes).toHaveLength(1);
    expect(sent?.gradient).toEqual(FULL_MASK.gradient);
    expect(sent?.raster).toEqual(RASTER);
    expect(sent?.inverted).toBe(true);
    expect(sent?.feather).toBe(0.3);
    expect(sent?.density).toBe(0.75);
  });

  it("keeps the gradient and raster when auto-skin is switched off", async () => {
    await useEditorStore.getState().setAutoSkinMask("layer-blur", false);

    expect(api.setLayerMask).toHaveBeenCalledTimes(1);
    const sent = lastSentMask();
    expect(sent).not.toBeNull();
    expect(sent?.raster).toEqual(RASTER);
    expect(sent?.gradient).toEqual(FULL_MASK.gradient);
    expect(sent?.autoSkin).toBe(false);
  });

  it("allows a gradient change", async () => {
    await useEditorStore.getState().setMaskGradient("layer-blur", null);
    expect(api.setLayerMask).toHaveBeenCalledTimes(1);
    expect(lastSentMask()?.raster).toEqual(RASTER);
  });
});
