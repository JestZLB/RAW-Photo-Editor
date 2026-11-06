/**
 * Status-bar save indicator state machine.
 *
 * Extracted from `editorStore.ts` as the third (and final) store split. It used
 * to be a 900 ms timer started before the write, which could report "saved"
 * while a slow disk was still writing. Now "saved" only appears once every
 * in-flight write settled and a quiet period passed.
 *
 * The state machine owns only a counter and a timer; the store supplies the
 * current status and a setter, which makes the slow-disk and failure cases
 * testable with fake timers without mounting a store.
 */

export type SaveState = "saving" | "saved" | "error";

export interface SaveIndicatorDeps {
  status: () => SaveState;
  setStatus: (next: SaveState) => void;
  /** Test seam; production uses 400 ms. */
  settleMs?: number;
}

export interface SaveIndicator {
  /** A document write left the renderer. */
  start: () => void;
  /** A write settled; `ok` is false when it rejected. */
  settle: (ok: boolean) => void;
  /** Explicit failure, e.g. a queued write or commit threw. */
  fail: () => void;
  /** In-flight write count, for tests and diagnostics. */
  pending: () => number;
}

/** Quiet period after the last write settles before claiming "saved". */
const SAVED_SETTLE_MS = 400;

export function createSaveIndicator(deps: SaveIndicatorDeps): SaveIndicator {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pendingWrites = 0;
  const settleMs = deps.settleMs ?? SAVED_SETTLE_MS;

  function clearTimer(): void {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function start(): void {
    pendingWrites += 1;
    clearTimer();
    if (deps.status() !== "saving") deps.setStatus("saving");
  }

  function settle(ok: boolean): void {
    pendingWrites = Math.max(0, pendingWrites - 1);
    if (!ok) {
      fail();
      return;
    }
    if (pendingWrites > 0) return;
    clearTimer();
    timer = setTimeout(() => {
      timer = null;
      if (pendingWrites > 0) return;
      // A failure stays visible until a later write succeeds again.
      if (deps.status() === "error") return;
      deps.setStatus("saved");
    }, settleMs);
  }

  function fail(): void {
    pendingWrites = 0;
    clearTimer();
    deps.setStatus("error");
  }

  return { start, settle, fail, pending: () => pendingWrites };
}
