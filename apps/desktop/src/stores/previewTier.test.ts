// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { DocumentState, LayerState, RenderPreviewRequest } from "../api/types";
import { DEFAULT_COLOR } from "../lib/params";
import { useEditorStore } from "./editorStore";
import { useSettingsStore } from "./settingsStore";
import { useToastStore } from "./toastStore";

/**
 * The dual-tier preview lives in module-level scheduler state, so it gets its own
 * file: Vitest isolates module instances per file, which keeps a pending timer
 * from one case from starving the next.
 */
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
    blur: null,
    mask: null,
    assetId: null,
    previewPath: null,
    createdAt: NOW,
    ...overrides,
  };
}

function documentWithSource(): DocumentState {
  return {
    id: "project-a",
    name: "portrait",
    collectionId: "collection-one",
    collectionName: "人像",
    schemaVersion: 20,
    revision: 1,
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
    layers: [layer({}), layer({ id: "layer-color", name: "调色", kind: "color" })],
    canUndo: false,
    canRedo: false,
    history: [],
  };
}

describe("interactive preview tier", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useToastStore.setState({ toasts: [] });
  });

  afterEach(() => {
    vi.useRealTimers();
    useEditorStore.setState({ doc: null, preview: null, previewLoading: false });
  });

  it("previews a smaller proxy while dragging and always returns to the configured size", async () => {
    const document = documentWithSource();
    const dimensions: number[] = [];
    const metaCalls: number[] = [];
    const api = {
      updateLayerParamsMeta: vi.fn(async () => {
        metaCalls.push(1);
        // The envelope-only response must never carry layers.
        return { ...document, layers: [] };
      }),
      commitChanges: vi.fn(async () => document),
      renderPreview: vi.fn(async (request: RenderPreviewRequest) => {
        dimensions.push(request.maxDimension ?? 0);
        return {
          path: `C:/cache/render-${request.maxDimension}.png`,
          width: 900,
          height: 600,
          revision: 1,
          durationMs: 1,
          cached: false,
          backend: "cpu",
          histogram: null,
        };
      }),
    } as unknown as EditorApi;

    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: document,
      preview: null,
      selectedLayerId: "layer-color",
      autosave: "saved",
    });
    useSettingsStore.setState({ previewMaxDimension: 1600 });

    useEditorStore.getState().editColor("layer-color", DEFAULT_COLOR);
    await vi.advanceTimersByTimeAsync(400);
    expect(metaCalls).toHaveLength(1);
    expect(dimensions[0]).toBe(900);

    // Nothing is committed here, so the settle timer alone must restore quality.
    await vi.advanceTimersByTimeAsync(600);
    expect(dimensions.at(-1)).toBe(1600);
  });

  it("uses the configured size directly when an edit is committed", async () => {
    const document = documentWithSource();
    const dimensions: number[] = [];
    const api = {
      updateLayerParams: vi.fn(async () => document),
      commitChanges: vi.fn(async () => document),
      renderPreview: vi.fn(async (request: RenderPreviewRequest) => {
        dimensions.push(request.maxDimension ?? 0);
        return {
          path: `C:/cache/render-${request.maxDimension}.png`,
          width: 1600,
          height: 800,
          revision: 1,
          durationMs: 1,
          cached: false,
          backend: "cpu",
          histogram: null,
        };
      }),
    } as unknown as EditorApi;

    useEditorStore.setState({
      api,
      mode: "tauri",
      doc: document,
      preview: null,
      selectedLayerId: "layer-color",
      autosave: "saved",
    });
    useSettingsStore.setState({ previewMaxDimension: 1600 });

    await useEditorStore.getState().commitColor("layer-color", DEFAULT_COLOR, "曝光");
    await vi.advanceTimersByTimeAsync(400);

    expect(dimensions.length).toBeGreaterThan(0);
    expect(dimensions.every((value) => value === 1600)).toBe(true);
  });
});
