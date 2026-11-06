/**
 * Preview scheduling: debounced re-render, stale-result protection and the
 * smaller "interactive" tier used while a slider is being dragged.
 *
 * Extracted from `editorStore.ts` as the second store split. The scheduler
 * owns only timers and an epoch counter; everything it needs from the store is
 * a state snapshot plus three narrow callbacks, so the rules can be tested
 * with fake timers without mounting a store.
 *
 * Behaviour matches the old inline implementation: 100 ms default debounce,
 * 900 px interactive tier, 420 ms settle that always returns to the configured
 * size, stale results discarded by epoch/revision, and a retry (90 ms) when a
 * newer edit arrived during a render.
 */

import type { EditorApi } from "../api/client";
import type { DocumentState, PreviewResult } from "../api/types";
import { useSettingsStore } from "./settingsStore";

/** Longest edge while dragging; see ADR-004 decision 18. */
export const INTERACTIVE_PREVIEW_MAX = 900;
/** Quiet period after the last live edit before the full-size render starts. */
const INTERACTIVE_SETTLE_MS = 420;

export interface PreviewSchedulerDeps {
  /** Read live state at render time, not when the render was scheduled. */
  snapshot: () => {
    api: EditorApi | null;
    doc: DocumentState | null;
    previewFullFrame: boolean;
  };
  setLoading: (loading: boolean) => void;
  setPreview: (result: PreviewResult) => void;
  onError: (error: unknown, fallback: string) => void;
  /** Test seam; production uses 420 ms. */
  interactiveSettleMs?: number;
}

export interface PreviewScheduler {
  schedule: (delay?: number) => void;
  noteInteractiveEdit: () => void;
  endInteractive: () => void;
  /** Exposed for tests and diagnostics. */
  isInteractive: () => boolean;
}

export function createPreviewScheduler(deps: PreviewSchedulerDeps): PreviewScheduler {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let running = false;
  let dirty = false;
  let epoch = 0;
  let interactive = false;
  let interactiveTimer: ReturnType<typeof setTimeout> | null = null;
  const settleMs = deps.interactiveSettleMs ?? INTERACTIVE_SETTLE_MS;

  async function run(requestedEpoch: number): Promise<void> {
    const { api, doc } = deps.snapshot();
    if (!api || !doc || !doc.source) {
      dirty = false;
      return;
    }
    running = true;
    dirty = false;
    deps.setLoading(true);
    const projectId = doc.id;
    const requestRevision = doc.revision;
    try {
      const settings = useSettingsStore.getState();
      const result = await api.renderPreview({
        projectId,
        maxDimension: interactive
          ? Math.min(settings.previewMaxDimension, INTERACTIVE_PREVIEW_MAX)
          : settings.previewMaxDimension,
        ignoreCrop: deps.snapshot().previewFullFrame,
      });
      // Re-read state after the await: the user may have switched photos.
      const current = deps.snapshot().doc;
      const stillSameProject = current?.id === projectId;
      const stale =
        requestedEpoch !== epoch ||
        !stillSameProject ||
        result.revision < requestRevision ||
        result.revision < (current?.revision ?? 0);
      if (!stale && current) {
        deps.setPreview(result);
      } else if (stillSameProject) {
        dirty = true;
      }
    } catch (error) {
      if (deps.snapshot().doc?.id === projectId) {
        deps.onError(error, "预览更新没有完成，画面仍保留上一次的结果。");
      }
    } finally {
      running = false;
      if (dirty) {
        schedule(90);
      } else {
        deps.setLoading(false);
      }
    }
  }

  function schedule(delay = 100): void {
    dirty = true;
    epoch += 1;
    if (running || timer !== null) return;
    timer = setTimeout(() => {
      timer = null;
      void run(epoch);
    }, delay);
  }

  /**
   * A live edit started. Previews drop to the interactive tier so the canvas
   * can follow the pointer; the settle timer guarantees a full-size render even
   * when no commit follows (e.g. a drag that ends on the same value).
   */
  function noteInteractiveEdit(): void {
    interactive = true;
    if (interactiveTimer !== null) clearTimeout(interactiveTimer);
    interactiveTimer = setTimeout(() => {
      interactiveTimer = null;
      if (!interactive) return;
      interactive = false;
      schedule(30);
    }, settleMs);
  }

  /** Leave the interactive tier immediately, e.g. when an edit is committed. */
  function endInteractive(): void {
    if (interactiveTimer !== null) {
      clearTimeout(interactiveTimer);
      interactiveTimer = null;
    }
    interactive = false;
  }

  return {
    schedule,
    noteInteractiveEdit,
    endInteractive,
    isInteractive: () => interactive,
  };
}
