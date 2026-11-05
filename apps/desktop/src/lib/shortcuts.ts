import type { ToolId } from "../stores/editorStore";

export type EditorShortcut = "undo" | "redo";

/** Unmodified keys, resolved centrally so the mapping stays testable. */
export type PlainAction =
  | { type: "tool"; tool: ToolId }
  | { type: "zoom"; direction: 1 | -1 }
  | { type: "fit" }
  | { type: "compare" }
  | { type: "delete-layer" }
  | { type: "brush-size"; direction: 1 | -1 }
  | { type: "cancel" };

const TOOL_KEYS: Record<string, ToolId> = {
  "1": "photo",
  "2": "crop",
  "3": "portrait",
  "4": "color",
  "5": "retouch",
  "6": "ai",
};

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable;
}

export function isMacPlatform(platform: string): boolean {
  return /mac|iphone|ipad|ipod/i.test(platform);
}

export function resolveModifierShortcut(event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}): EditorShortcut | null {
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return null;
  const key = event.key.toLowerCase();
  if (key === "z") return event.shiftKey ? "redo" : "undo";
  if (key === "y" && !event.shiftKey) return "redo";
  return null;
}

/**
 * Shortcuts without a modifier. `Shift` is allowed because `+` is typed with it
 * on most layouts; the keys themselves are unambiguous.
 */
export function resolvePlainShortcut(
  event: { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean },
  options: { brushActive: boolean },
): PlainAction | null {
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  const tool = TOOL_KEYS[event.key];
  if (tool) return { type: "tool", tool };
  switch (event.key) {
    case "+":
    case "=":
      return { type: "zoom", direction: 1 };
    case "-":
    case "_":
      return { type: "zoom", direction: -1 };
    case "0":
      return { type: "fit" };
    case " ":
      return { type: "compare" };
    case "Delete":
      return { type: "delete-layer" };
    case "Escape":
      return { type: "cancel" };
    case "[":
      return options.brushActive ? { type: "brush-size", direction: -1 } : null;
    case "]":
      return options.brushActive ? { type: "brush-size", direction: 1 } : null;
    default:
      return null;
  }
}

export function shortcutHint(mac: boolean): { undo: string; redo: string } {
  return mac ? { undo: "⌘ Z", redo: "⇧ ⌘ Z" } : { undo: "Ctrl Z", redo: "Ctrl ⇧ Z" };
}
