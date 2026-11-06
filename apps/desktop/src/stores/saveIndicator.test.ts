import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSaveIndicator, type SaveIndicator, type SaveState } from "./saveIndicator";

describe("save indicator", () => {
  let state: SaveState;
  let indicator: SaveIndicator;

  beforeEach(() => {
    vi.useFakeTimers();
    state = "saved";
    indicator = createSaveIndicator({
      status: () => state,
      setStatus: (next) => {
        state = next;
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows saving as soon as a write starts", () => {
    indicator.start();
    expect(state).toBe("saving");
    expect(indicator.pending()).toBe(1);
  });

  it("never claims saved while a slow write is still in flight", () => {
    // Regression: the old 900 ms timer started before the write and reported
    // success after 900 ms even if nothing had been written yet.
    indicator.start();
    vi.advanceTimersByTime(5_000);
    expect(state).toBe("saving");
  });

  it("waits for the quiet period after the last write settles", () => {
    indicator.start();
    indicator.settle(true);
    vi.advanceTimersByTime(399);
    expect(state).toBe("saving");
    vi.advanceTimersByTime(1);
    expect(state).toBe("saved");
  });

  it("stays saving until every concurrent write finished", () => {
    indicator.start();
    indicator.start();
    indicator.settle(true);
    vi.advanceTimersByTime(500);
    expect(state).toBe("saving");

    indicator.settle(true);
    vi.advanceTimersByTime(400);
    expect(state).toBe("saved");
    expect(indicator.pending()).toBe(0);
  });

  it("shows error when a write rejects", () => {
    indicator.start();
    indicator.settle(false);
    expect(state).toBe("error");
  });

  it("does not let a concurrent success overwrite an error", () => {
    indicator.start();
    indicator.start();
    indicator.settle(false);
    indicator.settle(true);
    vi.advanceTimersByTime(1_000);
    expect(state).toBe("error");
  });

  it("recovers to saved after a later write succeeds", () => {
    indicator.start();
    indicator.settle(false);
    expect(state).toBe("error");

    indicator.start();
    indicator.settle(true);
    vi.advanceTimersByTime(400);
    expect(state).toBe("saved");
  });
});
