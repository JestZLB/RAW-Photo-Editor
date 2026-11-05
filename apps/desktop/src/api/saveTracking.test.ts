import { describe, expect, it, vi } from "vitest";
import type { EditorApi } from "./client";
import { trackDocumentWrites } from "./saveTracking";

function hooks() {
  const calls: string[] = [];
  return {
    calls,
    start: () => calls.push("start"),
    settle: (ok: boolean) => calls.push(ok ? "settle:ok" : "settle:error"),
  };
}

function fakeApi(overrides: Partial<EditorApi> = {}): EditorApi {
  return {
    mode: "demo",
    renderPreview: vi.fn(async () => ({ path: "preview.png" })),
    commitChanges: vi.fn(async () => ({ id: "project-a" })),
    updateLayerParamsMeta: vi.fn(async () => ({ id: "project-a", layers: [] })),
    cancelAiJob: vi.fn(async () => undefined),
    ...overrides,
  } as unknown as EditorApi;
}

describe("trackDocumentWrites", () => {
  it("reports start before settle, in that order", async () => {
    const h = hooks();
    const api = trackDocumentWrites(fakeApi(), h);

    const state = await api.commitChanges({ projectId: "project-a", label: "曝光" });

    expect(state).toEqual({ id: "project-a" });
    expect(h.calls).toEqual(["start", "settle:ok"]);
  });

  it("reports a failure and rethrows the original error", async () => {
    const h = hooks();
    const failure = new Error("disk full");
    const api = trackDocumentWrites(
      fakeApi({ setCrop: vi.fn(async () => Promise.reject(failure)) }),
      h,
    );

    await expect(
      api.setCrop({
        projectId: "project-a",
        crop: { x: 0, y: 0, width: 1, height: 1 },
      }),
    ).rejects.toBe(failure);
    expect(h.calls).toEqual(["start", "settle:error"]);
  });

  it("reports a synchronously thrown write as a failure", async () => {
    const h = hooks();
    const api = trackDocumentWrites(
      fakeApi({
        deleteLayer: () => {
          throw new Error("not allowed");
        },
      }),
      h,
    );

    expect(() => api.deleteLayer({ projectId: "p", layerId: "l" })).toThrow("not allowed");
    expect(h.calls).toEqual(["start", "settle:error"]);
  });

  it("leaves read-only and long-running commands untracked", async () => {
    const h = hooks();
    const api = trackDocumentWrites(fakeApi(), h);

    await api.renderPreview({ projectId: "project-a" });
    await api.cancelAiJob({ jobId: "job-1" });

    expect(h.calls).toEqual([]);
  });

  it("keeps `this` bound for class-based implementations", async () => {
    const h = hooks();
    class DemoApi {
      readonly mode = "demo" as const;
      constructor(private readonly prefix: string) {}
      async commitChanges(): Promise<string> {
        return `${this.prefix}-saved`;
      }
    }
    const api = trackDocumentWrites(new DemoApi("demo") as unknown as EditorApi, h);

    await expect(
      api.commitChanges({ projectId: "project-a", label: "调整" }),
    ).resolves.toBe("demo-saved");
    expect(h.calls).toEqual(["start", "settle:ok"]);
  });
});
