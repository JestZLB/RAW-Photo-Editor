// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { EditorApi } from "../api/client";
import { useEditorStore } from "./editorStore";

/**
 * The status bar must never claim "saved" while a write is still in flight.
 * These cases drive a write through the store's tracked API and hold the promise
 * open, which is exactly the situation the previous 900 ms timer got wrong.
 */
const gate = vi.hoisted(() => ({
  resolve: null as null | (() => void),
  reject: null as null | ((error: unknown) => void),
  promise: undefined as unknown as Promise<void>,
}));

function openGate(): void {
  gate.promise = new Promise<void>((resolve, reject) => {
    gate.resolve = resolve;
    gate.reject = reject;
  });
}

vi.mock("../api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/client")>();
  const api = {
    mode: "demo",
    getAppInfo: async () => ({
      version: "0.0.0",
      backend: "cpu",
      gpuAvailable: false,
      rawAvailable: false,
      faceModelAvailable: false,
      workspaceDir: "/tmp/luma",
      supportedImageFormats: ["jpg"],
    }),
    listProjects: async () => [],
    listCollections: async () => [],
    listProviders: async () => [],
    // Only the write command needs to be controllable.
    commitChanges: () => gate.promise,
  } as unknown as EditorApi;
  return { ...actual, getApi: () => api };
});

describe("save indicator", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    openGate();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("stays on 'saving' until the write settles, however long it takes", async () => {
    await useEditorStore.getState().bootstrap();
    const api = useEditorStore.getState().api;
    expect(api).not.toBeNull();

    const write = api!.commitChanges({ projectId: "project-a", label: "调整" });
    expect(useEditorStore.getState().autosave).toBe("saving");

    // The old implementation flipped to "saved" after 900 ms regardless.
    await vi.advanceTimersByTimeAsync(2000);
    expect(useEditorStore.getState().autosave).toBe("saving");

    gate.resolve?.();
    await write;
    await vi.advanceTimersByTimeAsync(500);
    expect(useEditorStore.getState().autosave).toBe("saved");
  });

  it("keeps a failure visible instead of replacing it with 'saved'", async () => {
    await useEditorStore.getState().bootstrap();
    const api = useEditorStore.getState().api;

    const write = api!
      .commitChanges({ projectId: "project-a", label: "调整" })
      .catch(() => undefined);
    gate.reject?.(new Error("disk full"));
    await write;

    expect(useEditorStore.getState().autosave).toBe("error");
    await vi.advanceTimersByTimeAsync(2000);
    expect(useEditorStore.getState().autosave).toBe("error");
  });
});
