// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { AddLayerRequest, DocumentState, LayerState } from "../api/types";
import { DEFAULT_COLOR } from "../lib/params";
import { useEditorStore } from "./editorStore";
import { useToastStore } from "./toastStore";

const NOW = "2026-11-06T00:00:00.000Z";
const RASTER_ASSET = "0f8fad5b-d9cb-469f-a165-70867728950e";

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

function baseDocument(): DocumentState {
  return {
    id: "project-a",
    name: "portrait",
    collectionId: "collection-one",
    collectionName: "人像",
    schemaVersion: 21,
    revision: 2,
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
        id: "layer-subject",
        name: "背景虚化",
        kind: "blur",
        blur: { radius: 0.03 },
        // Only a raster mask: this is what segmentation produces.
        mask: {
          strokes: [],
          autoSkin: false,
          inverted: false,
          feather: 0,
          density: 1,
          raster: {
            assetId: RASTER_ASSET,
            threshold: 0.5,
            softness: 0.12,
            source: "onnx:foreground",
          },
        },
      }),
    ],
    canUndo: true,
    canRedo: false,
    history: ["添加背景虚化"],
  };
}

describe("one-click looks", () => {
  let api: EditorApi;
  let created: AddLayerRequest[];

  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    created = [];
    api = {
      addLayer: vi.fn(async (req: AddLayerRequest) => {
        created.push(req);
        const doc = baseDocument();
        doc.layers.push(
          layer({
            id: "layer-look",
            name: req.name ?? "调色调整",
            kind: "color",
            color: { ...DEFAULT_COLOR },
            mask: req.mask ?? null,
          }),
        );
        doc.revision += 1;
        return doc;
      }),
      updateLayerParamsMeta: vi.fn(async () => ({
        ...baseDocument(),
        revision: 4,
        layers: [],
      })),
      updateLayerParams: vi.fn(async () => baseDocument()),
      commitChanges: vi.fn(async () => ({ ...baseDocument(), revision: 5 })),
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
      doc: baseDocument(),
      selectedLayerId: "layer-subject",
      preview: null,
      autosave: "saved",
    });
  });

  it("desaturates the background by copying the mask inverted", async () => {
    await useEditorStore.getState().applyLook("layer-subject", "desaturateBackground");

    expect(created).toHaveLength(1);
    expect(created[0].kind).toBe("color");
    expect(created[0].name).toBe("背景去色");
    // Background = outside the subject.
    expect(created[0].mask?.inverted).toBe(true);
    expect(created[0].mask?.raster?.assetId).toBe(RASTER_ASSET);

    expect(api.updateLayerParamsMeta).toHaveBeenCalledWith(
      expect.objectContaining({
        layerId: "layer-look",
        color: expect.objectContaining({ saturation: -100 }),
      }),
    );
    expect(api.commitChanges).toHaveBeenCalledWith(
      expect.objectContaining({ label: "背景去色" }),
    );
  });

  it("brightens the subject with the mask as-is", async () => {
    await useEditorStore.getState().applyLook("layer-subject", "brightenSubject");

    expect(created[0].name).toBe("主体提亮");
    expect(created[0].mask?.inverted).toBe(false);
    expect(api.updateLayerParamsMeta).toHaveBeenCalledWith(
      expect.objectContaining({
        color: expect.objectContaining({ exposure: 0.35 }),
      }),
    );
    expect(api.commitChanges).toHaveBeenCalledWith(
      expect.objectContaining({ label: "主体提亮" }),
    );
  });

  it("refuses to guess when the layer has no mask", async () => {
    await useEditorStore.getState().applyLook("layer-base", "brightenSubject");

    expect(api.addLayer).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts.at(-1)?.title).toBe("没有可用的蒙版");
  });
});
