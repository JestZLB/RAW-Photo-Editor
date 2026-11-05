import { describe, expect, it } from "vitest";
import { resolvePlainShortcut } from "./shortcuts";

const key = (value: string, extra: Record<string, boolean> = {}) => ({
  key: value,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  ...extra,
});

describe("plain editor shortcuts", () => {
  it("maps the number row to the tool rail order", () => {
    const tools = ["1", "2", "3", "4", "5", "6"].map((value) => {
      const action = resolvePlainShortcut(key(value), { brushActive: false });
      return action?.type === "tool" ? action.tool : null;
    });
    expect(tools).toEqual(["photo", "crop", "portrait", "color", "retouch", "ai"]);
    expect(resolvePlainShortcut(key("7"), { brushActive: false })).toBeNull();
  });

  it("zooms with plus and minus and fits with zero", () => {
    expect(resolvePlainShortcut(key("+"), { brushActive: false })).toEqual({
      type: "zoom",
      direction: 1,
    });
    expect(resolvePlainShortcut(key("="), { brushActive: false })).toEqual({
      type: "zoom",
      direction: 1,
    });
    expect(resolvePlainShortcut(key("-"), { brushActive: false })).toEqual({
      type: "zoom",
      direction: -1,
    });
    expect(resolvePlainShortcut(key("0"), { brushActive: false })).toEqual({ type: "fit" });
  });

  it("holds space to compare and deletes the selected layer", () => {
    expect(resolvePlainShortcut(key(" "), { brushActive: false })).toEqual({ type: "compare" });
    expect(resolvePlainShortcut(key("Delete"), { brushActive: false })).toEqual({
      type: "delete-layer",
    });
    expect(resolvePlainShortcut(key("Escape"), { brushActive: false })).toEqual({ type: "cancel" });
  });

  it("only resizes the brush while it is active", () => {
    expect(resolvePlainShortcut(key("]"), { brushActive: false })).toBeNull();
    expect(resolvePlainShortcut(key("]"), { brushActive: true })).toEqual({
      type: "brush-size",
      direction: 1,
    });
    expect(resolvePlainShortcut(key("["), { brushActive: true })).toEqual({
      type: "brush-size",
      direction: -1,
    });
  });

  it("ignores modified keys so browser and app shortcuts win", () => {
    for (const modifier of ["ctrlKey", "metaKey", "altKey"]) {
      expect(
        resolvePlainShortcut(key("1", { [modifier]: true }), { brushActive: false }),
      ).toBeNull();
    }
  });
});
