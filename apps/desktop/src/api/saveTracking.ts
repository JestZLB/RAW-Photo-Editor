import type { EditorApi } from "./client";

/**
 * Commands that persist the document. Only these drive the status-bar save
 * indicator: queries, previews, exports and long-running AI jobs have their own
 * progress UI and must never claim the document is being saved.
 */
const WRITE_COMMANDS = new Set<string>([
  "createProject",
  "importPhoto",
  "importPhotoBytes",
  "updateLayerParams",
  "updateLayerParamsMeta",
  "updateLayer",
  "commitChanges",
  "setLayerMask",
  "setCrop",
  "addLayer",
  "duplicateLayer",
  "deleteLayer",
  "moveLayer",
  "undo",
  "redo",
  "applyAiResult",
  "syncColorAdjustments",
  "renameProject",
  "renameCollection",
  "deleteProject",
]);

export interface SaveTrackingHooks {
  /** A document write left the renderer. */
  start(): void;
  /** A write settled; `ok` is false when it rejected. */
  settle(ok: boolean): void;
}

/**
 * Wrap an API so the save indicator follows write completion instead of a timer
 * guess. Every mutating call reports start → settle, which lets the store show
 * "saved" only after the last in-flight write actually finished.
 *
 * Methods are invoked with `apply(target, …)` on purpose: class-based
 * implementations (`DemoBackend`) call `this.requireProject(...)`, so a detached
 * function call would throw.
 */
export function trackDocumentWrites(api: EditorApi, hooks: SaveTrackingHooks): EditorApi {
  return new Proxy(api, {
    get(target, property, receiver) {
      const value: unknown = Reflect.get(target, property, receiver);
      if (typeof value !== "function" || !WRITE_COMMANDS.has(String(property))) {
        return value;
      }
      const write = value as (...args: unknown[]) => Promise<unknown>;
      return (...args: unknown[]): Promise<unknown> => {
        hooks.start();
        let pending: Promise<unknown>;
        try {
          pending = Promise.resolve(write.apply(target, args));
        } catch (error) {
          hooks.settle(false);
          throw error;
        }
        return pending.then(
          (result) => {
            hooks.settle(true);
            return result;
          },
          (error: unknown) => {
            hooks.settle(false);
            throw error;
          },
        );
      };
    },
  }) as EditorApi;
}
