/**
 * Coalesces high-frequency slider and layer edits into debounced IPC writes.
 *
 * This used to live inside `editorStore.ts` as a handful of module-level
 * variables. It is a self-contained subsystem: the only things it needs from
 * the store are a state snapshot and four callbacks, so keeping it here makes
 * the store's remaining job (state + actions) easier to follow and lets the
 * coalescing rules be tested without mounting a store.
 *
 * Behaviour is intentionally identical to the old inline implementation:
 * 70 ms debounce, per-layer merge, serialised drain chain, skip entries whose
 * project is no longer open, and a re-render once writes land.
 */

import type { EditorApi } from "../api/client";
import type {
  BlurParams,
  ColorParams,
  DocumentState,
  LayerPatch,
  PortraitParams,
} from "../api/types";

export interface ParamPatch {
  color?: ColorParams;
  portrait?: PortraitParams;
  blur?: BlurParams;
}

interface ParamQueueItem extends ParamPatch {
  projectId: string;
}

interface LayerPatchQueueItem {
  projectId: string;
  patch: LayerPatch;
}

export interface EditQueueDeps {
  /** Read live state at flush time, not when the edit was queued. */
  snapshot: () => { api: EditorApi | null; doc: DocumentState | null };
  mergeServerMeta: (next: DocumentState) => void;
  markSaveFailed: () => void;
  onError: (error: unknown, fallback: string) => void;
  /** Re-render after pending writes land. */
  schedulePreview: (delay?: number) => void;
  /** Override the debounce for tests; production uses 70 ms. */
  debounceMs?: number;
}

export interface EditQueue {
  queueParams: (layerId: string, patch: ParamPatch) => void;
  queueLayerPatch: (layerId: string, patch: LayerPatch) => void;
  /** Cancel the debounce and await every queued write. */
  flush: () => Promise<void>;
  /** Pending entry count, used by tests and diagnostics. */
  pending: () => number;
}

const SYNC_DEBOUNCE_MS = 70;

export function createEditQueue(deps: EditQueueDeps): EditQueue {
  let pendingParams = new Map<string, ParamQueueItem>();
  let pendingLayerPatches = new Map<string, LayerPatchQueueItem>();
  let syncTimer: ReturnType<typeof setTimeout> | null = null;
  let syncRunning = false;
  let syncChain: Promise<void> = Promise.resolve();
  const debounceMs = deps.debounceMs ?? SYNC_DEBOUNCE_MS;

  async function drain(): Promise<void> {
    if (syncRunning) return;
    syncRunning = true;
    const { api } = deps.snapshot();
    if (!api) {
      pendingParams.clear();
      pendingLayerPatches.clear();
      syncRunning = false;
      return;
    }
    try {
      while (pendingLayerPatches.size > 0 || pendingParams.size > 0) {
        const patches = [...pendingLayerPatches.entries()];
        pendingLayerPatches.clear();
        for (const [key, item] of patches) {
          const doc = deps.snapshot().doc;
          if (!doc || doc.id !== item.projectId) continue;
          const layerId = key.slice(key.indexOf(":") + 1);
          try {
            const next = await api.updateLayer({
              projectId: item.projectId,
              layerId,
              patch: item.patch,
            });
            deps.mergeServerMeta(next);
          } catch (error) {
            deps.markSaveFailed();
            deps.onError(error, "图层调整没有保存，请重试。");
          }
        }

        const params = [...pendingParams.entries()];
        pendingParams.clear();
        for (const [key, item] of params) {
          const doc = deps.snapshot().doc;
          if (!doc || doc.id !== item.projectId) continue;
          const layerId = key.slice(key.indexOf(":") + 1);
          try {
            // Drag path: only the envelope is merged here, so the full document
            // (every mask and retouch point) must not travel back over IPC.
            const next = await api.updateLayerParamsMeta({
              projectId: item.projectId,
              layerId,
              ...(item.color ? { color: item.color } : {}),
              ...(item.portrait ? { portrait: item.portrait } : {}),
              ...(item.blur ? { blur: item.blur } : {}),
            });
            deps.mergeServerMeta(next);
          } catch (error) {
            deps.markSaveFailed();
            deps.onError(error, "调整没有保存，请再试一次。");
          }
        }
      }
      deps.schedulePreview(70);
    } finally {
      syncRunning = false;
      if (pendingLayerPatches.size > 0 || pendingParams.size > 0) {
        syncChain = syncChain.then(drain).catch(() => undefined);
      }
    }
  }

  function scheduleSync(): void {
    if (syncTimer !== null) return;
    syncTimer = setTimeout(() => {
      syncTimer = null;
      syncChain = syncChain.then(drain).catch(() => undefined);
    }, debounceMs);
  }

  return {
    queueParams(layerId, patch) {
      const doc = deps.snapshot().doc;
      if (!doc) return;
      const key = `${doc.id}:${layerId}`;
      const existing = pendingParams.get(key);
      pendingParams.set(key, {
        projectId: doc.id,
        color: patch.color ?? existing?.color,
        portrait: patch.portrait ?? existing?.portrait,
        blur: patch.blur ?? existing?.blur,
      });
      scheduleSync();
    },

    queueLayerPatch(layerId, patch) {
      const doc = deps.snapshot().doc;
      if (!doc) return;
      const key = `${doc.id}:${layerId}`;
      const existing = pendingLayerPatches.get(key);
      pendingLayerPatches.set(key, {
        projectId: doc.id,
        patch: { ...existing?.patch, ...patch },
      });
      scheduleSync();
    },

    flush() {
      if (syncTimer !== null) {
        clearTimeout(syncTimer);
        syncTimer = null;
      }
      syncChain = syncChain.then(drain).catch(() => undefined);
      return syncChain;
    },

    pending() {
      return pendingParams.size + pendingLayerPatches.size;
    },
  };
}
