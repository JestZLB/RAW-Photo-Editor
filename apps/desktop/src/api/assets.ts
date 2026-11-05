import { convertFileSrc } from "@tauri-apps/api/core";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && typeof window.__TAURI_INTERNALS__ !== "undefined";
}

const PASSTHROUGH_PROTOCOL = /^(?:blob:|data:|https?:|file:)/i;

/**
 * Converts a backend path into something an <img> can display.
 * Browser-friendly URLs are passed through; plain filesystem paths only get
 * converted when we are actually running inside Tauri.
 */
export function resolveAssetUrl(path: string | null | undefined): string | null {
  if (!path) return null;
  if (PASSTHROUGH_PROTOCOL.test(path)) return path;
  if (isTauriRuntime()) {
    try {
      return convertFileSrc(path);
    } catch {
      return null;
    }
  }
  return path;
}
