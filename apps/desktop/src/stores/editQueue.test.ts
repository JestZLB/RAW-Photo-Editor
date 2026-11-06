import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { DocumentState } from "../api/types";
import { DEFAULT_COLOR } from "../lib/params";
import { createEditQueue, type EditQueueDeps } from "./editQueue";

const exposure = (value: number) => ({ ...DEFAULT_COLOR, exposure: value });

const NOW = "2026-11-06T00:00:00.000Z";

function documentWith(id = "project-a"): DocumentState {
  return {
    id,
    name: "portrait",
    collectionId: "collection-one",
    collectionName: "人像",
    schemaVersion: 21,
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
    layers: [],
    canUndo: true,
    canRedo: false,
    history: [],
  };
}

describe("edit queue", () => {
  let doc: DocumentState;
  let api: EditorApi;
  let deps: EditQueueDeps;
  let markSaveFailed: ReturnType<typeof vi.fn>;
  let onError: ReturnType<typeof vi.fn>;
  let schedulePreview: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    doc = documentWith();
    markSaveFailed = vi.fn();
    onError = vi.fn();
    schedulePreview = vi.fn();
    api = {
      updateLayer: vi.fn(async () => documentWith(doc.id)),
      updateLayerParamsMeta: vi.fn(async () => ({ ...documentWith(doc.id), layers: [] })),
    } as unknown as EditorApi;
    deps = {
      snapshot: () => ({ api, doc }),
      mergeServerMeta: vi.fn(),
      markSaveFailed,
      onError,
      schedulePreview,
      debounceMs: 10_000, // never fires during a test; we always flush() explicitly
    };
  });

  it("merges repeated edits to one layer into a single lightweight write", async () => {
    const queue = createEditQueue(deps);
    queue.queueParams("layer-color", { color: exposure(0.1) });
    queue.queueParams("layer-color", { color: exposure(0.7) });
    queue.queueParams("layer-color", { blur: { radius: 0.04 } });
    expect(queue.pending()).toBe(1);

    await queue.flush();

    expect(api.updateLayerParamsMeta).toHaveBeenCalledTimes(1);
    expect(api.updateLayerParamsMeta).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-a",
        layerId: "layer-color",
        color: exposure(0.7),
        blur: { radius: 0.04 },
      }),
    );
    expect(queue.pending()).toBe(0);
    expect(deps.mergeServerMeta).toHaveBeenCalledTimes(1);
  });

  it("merges layer patches field by field", async () => {
    const queue = createEditQueue(deps);
    queue.queueLayerPatch("layer-1", { opacity: 0.6 });
    queue.queueLayerPatch("layer-1", { visible: false });

    await queue.flush();

    expect(api.updateLayer).toHaveBeenCalledTimes(1);
    expect(api.updateLayer).toHaveBeenCalledWith({
      projectId: "project-a",
      layerId: "layer-1",
      patch: { opacity: 0.6, visible: false },
    });
  });

  it("drops entries whose project is no longer open", async () => {
    const queue = createEditQueue(deps);
    queue.queueParams("layer-1", { blur: { radius: 0.02 } });
    // User switched photos before the write landed.
    doc = documentWith("project-b");

    await queue.flush();

    expect(api.updateLayerParamsMeta).not.toHaveBeenCalled();
    expect(queue.pending()).toBe(0);
  });

  it("reports a failed write but still sends the remaining entries", async () => {
    vi.mocked(api.updateLayerParamsMeta)
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce({ ...documentWith(), layers: [] });
    const queue = createEditQueue(deps);
    queue.queueParams("layer-a", { color: exposure(0.2) });
    queue.queueParams("layer-b", { color: exposure(0.9) });

    await queue.flush();

    expect(api.updateLayerParamsMeta).toHaveBeenCalledTimes(2);
    expect(markSaveFailed).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledWith(
      expect.any(Error),
      "调整没有保存，请再试一次。",
    );
  });

  it("clears the queue when no API is available", async () => {
    const queue = createEditQueue({
      ...deps,
      snapshot: () => ({ api: null, doc }),
    });
    queue.queueParams("layer-1", { blur: { radius: 0.02 } });

    await queue.flush();

    expect(queue.pending()).toBe(0);
  });

  it("always requests a preview after draining", async () => {
    const queue = createEditQueue(deps);
    await queue.flush();
    expect(schedulePreview).toHaveBeenCalledWith(70);
  });
});
