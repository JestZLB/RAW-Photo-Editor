import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import type { DocumentState, PreviewResult } from "../api/types";
import { createPreviewScheduler, type PreviewSchedulerDeps } from "./previewScheduler";
import { useSettingsStore } from "./settingsStore";

const NOW = "2026-11-06T00:00:00.000Z";

function doc(id = "project-a", revision = 1): DocumentState {
  return {
    id,
    name: "portrait",
    collectionId: "collection-one",
    collectionName: "人像",
    schemaVersion: 21,
    revision,
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

function result(revision: number): PreviewResult {
  return {
    path: `C:/cache/render-${revision}.png`,
    width: 1600,
    height: 800,
    revision,
    durationMs: 1,
    cached: false,
    backend: "cpu",
    histogram: null,
  };
}

describe("preview scheduler", () => {
  let currentDoc: DocumentState | null;
  let api: EditorApi;
  let setLoading: ReturnType<typeof vi.fn>;
  let setPreview: ReturnType<typeof vi.fn>;
  let onError: ReturnType<typeof vi.fn>;
  let deps: PreviewSchedulerDeps;

  beforeEach(() => {
    vi.useFakeTimers();
    useSettingsStore.setState({ previewMaxDimension: 1600 });
    currentDoc = doc();
    setLoading = vi.fn();
    setPreview = vi.fn();
    onError = vi.fn();
    api = {
      renderPreview: vi.fn(async () => result(1)),
    } as unknown as EditorApi;
    deps = {
      snapshot: () => ({ api, doc: currentDoc, previewFullFrame: false }),
      setLoading,
      setPreview,
      onError,
    };
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("uses the 900px interactive tier while a drag is active", async () => {
    const scheduler = createPreviewScheduler(deps);
    scheduler.noteInteractiveEdit();
    scheduler.schedule(40);

    await vi.advanceTimersByTimeAsync(40);

    expect(api.renderPreview).toHaveBeenCalledWith(
      expect.objectContaining({ maxDimension: 900, ignoreCrop: false }),
    );
    expect(scheduler.isInteractive()).toBe(true);
  });

  it("always returns to the configured tier after the settle timer", async () => {
    const scheduler = createPreviewScheduler(deps);
    scheduler.noteInteractiveEdit();
    scheduler.schedule(40);
    await vi.advanceTimersByTimeAsync(40);
    vi.mocked(api.renderPreview).mockClear();

    // No commit follows the drag: the settle timer alone must re-render at the
    // full configured size.
    await vi.advanceTimersByTimeAsync(420);
    await vi.advanceTimersByTimeAsync(30);

    expect(scheduler.isInteractive()).toBe(false);
    expect(api.renderPreview).toHaveBeenCalledWith(
      expect.objectContaining({ maxDimension: 1600 }),
    );
  });

  it("leaves the interactive tier immediately when an edit is committed", async () => {
    const scheduler = createPreviewScheduler(deps);
    scheduler.noteInteractiveEdit();
    scheduler.endInteractive();
    scheduler.schedule(40);

    await vi.advanceTimersByTimeAsync(40);

    expect(scheduler.isInteractive()).toBe(false);
    expect(api.renderPreview).toHaveBeenCalledWith(
      expect.objectContaining({ maxDimension: 1600 }),
    );
  });

  it("discards a stale result and retries with the newer epoch", async () => {
    let resolveFirst!: (value: PreviewResult) => void;
    const first = new Promise<PreviewResult>((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(api.renderPreview)
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce(result(2));
    const scheduler = createPreviewScheduler(deps);

    scheduler.schedule(10);
    await vi.advanceTimersByTimeAsync(10);
    // A newer edit arrives while the first render is still in flight.
    scheduler.schedule(10);
    resolveFirst(result(1));
    await vi.advanceTimersByTimeAsync(0);

    expect(setPreview).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(90);
    expect(setPreview).toHaveBeenCalledTimes(1);
    expect(setPreview).toHaveBeenCalledWith(result(2));
  });

  it("never writes a preview for a project that was switched away", async () => {
    let resolveFirst!: (value: PreviewResult) => void;
    const first = new Promise<PreviewResult>((resolve) => {
      resolveFirst = resolve;
    });
    vi.mocked(api.renderPreview).mockReturnValueOnce(first);
    const scheduler = createPreviewScheduler(deps);

    scheduler.schedule(10);
    await vi.advanceTimersByTimeAsync(10);
    currentDoc = doc("project-b");
    resolveFirst(result(1));
    await vi.advanceTimersByTimeAsync(0);

    expect(setPreview).not.toHaveBeenCalled();
  });

  it("reports failures and clears the loading flag without replacing the image", async () => {
    vi.mocked(api.renderPreview).mockRejectedValueOnce(new Error("render failed"));
    const scheduler = createPreviewScheduler(deps);

    scheduler.schedule(10);
    await vi.advanceTimersByTimeAsync(10);

    expect(onError).toHaveBeenCalledWith(
      expect.any(Error),
      "预览更新没有完成，画面仍保留上一次的结果。",
    );
    expect(setPreview).not.toHaveBeenCalled();
    expect(setLoading).toHaveBeenLastCalledWith(false);
  });

  it("does nothing when there is no source photo", async () => {
    currentDoc = { ...doc(), source: null };
    const scheduler = createPreviewScheduler(deps);

    scheduler.schedule(10);
    await vi.advanceTimersByTimeAsync(10);

    expect(api.renderPreview).not.toHaveBeenCalled();
  });
});
