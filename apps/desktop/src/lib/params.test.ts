import { describe, expect, it } from "vitest";

import { clamp, clamp01, round, smoothstep } from "./clamp";
import { formatBytes, formatDimensions, formatSigned, formatZoom, shortenPath } from "./format";
import {
  BASE_PORTRAIT_EFFECTS,
  BEAUTY_PORTRAIT_EFFECTS,
  BODY_PORTRAIT_EFFECTS,
  COLOR_PRESETS,
  DEFAULT_COLOR,
  DEFAULT_PORTRAIT,
  FINE_PORTRAIT_EFFECTS,
  PORTRAIT_EFFECTS,
  PORTRAIT_PRESETS,
  WRINKLE_ZONE_EFFECTS,
  enabledPortraitEffects,
  isDefaultColor,
} from "./params";
import { resolveModifierShortcut } from "./shortcuts";

describe("clamp", () => {
  it("keeps values inside the range", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-3, 0, 10)).toBe(0);
    expect(clamp(42, 0, 10)).toBe(10);
  });

  it("falls back to the minimum for non-finite input", () => {
    expect(clamp(Number.NaN, 2, 8)).toBe(2);
  });

  it("clamps to the unit interval", () => {
    expect(clamp01(0.4)).toBe(0.4);
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(3)).toBe(1);
  });

  it("rounds to a fixed number of digits", () => {
    expect(round(1.2345, 2)).toBe(1.23);
    expect(round(7.6)).toBe(8);
  });

  it("smoothstep eases between edges", () => {
    expect(smoothstep(0, 1, 0)).toBe(0);
    expect(smoothstep(0, 1, 0.5)).toBe(0.5);
    expect(smoothstep(0, 1, 1)).toBe(1);
    expect(smoothstep(0, 1, -4)).toBe(0);
    expect(smoothstep(0, 1, 9)).toBe(1);
  });
});

describe("format helpers", () => {
  it("formats signed values", () => {
    expect(formatSigned(12)).toBe("+12");
    expect(formatSigned(-4)).toBe("-4");
    expect(formatSigned(0)).toBe("0");
    expect(formatSigned(0.12, 2)).toBe("+0.12");
  });

  it("formats bytes with sensible units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });

  it("formats dimensions, zoom and long paths", () => {
    expect(formatDimensions(1920, 1080)).toBe("1920 × 1080");
    expect(formatZoom(74.6)).toBe("75%");
    const shortened = shortenPath("C:/photos/very-long-folder-name/session-portrait.jpg", 24);
    expect(shortened).toHaveLength(24);
    expect(shortened).toContain("…");
  });
});

describe("editor defaults and presets", () => {
  it("starts from neutral color values", () => {
    expect(isDefaultColor(DEFAULT_COLOR)).toBe(true);
    expect(isDefaultColor({ ...DEFAULT_COLOR, exposure: 0.2 })).toBe(false);
  });

  it("keeps portrait effects conservative by default", () => {
    expect(DEFAULT_PORTRAIT.smooth).toBe(0);
    expect(DEFAULT_PORTRAIT.evenSkin).toBe(0);
    expect(DEFAULT_PORTRAIT.brighten).toBe(0);
    expect(DEFAULT_PORTRAIT.bodySmooth).toBe(0);
    expect(DEFAULT_PORTRAIT.intensity).toBe(78);
    expect(DEFAULT_PORTRAIT.texture).toBe(70);
  });

  it("keeps fine-repair effects off by default", () => {
    const disabledKeys = ["eyeEnhance", "teethWhiten", "blemish", "wrinkle", "darkCircle", "mole", "freckle", "birthmark", "tattoo"] as const;
    for (const key of disabledKeys) {
      expect(DEFAULT_PORTRAIT[key]).toBe(0);
    }
    for (const preset of PORTRAIT_PRESETS.filter((item) =>
      ["natural", "clear", "soft"].includes(item.id),
    )) {
      for (const key of disabledKeys) {
        expect(preset.settings[key]).toBe(0);
      }
    }
    const stage = PORTRAIT_PRESETS.find((item) => item.id === "stage");
    expect(stage?.settings.eyeEnhance).toBeGreaterThan(0);
    expect(stage?.settings.lipTint).toBeGreaterThan(0);
  });

  it("defines portrait presets inside the 0-100 range", () => {
    expect(PORTRAIT_PRESETS.map((preset) => preset.id)).toEqual([
      "natural",
      "clear",
      "soft",
      "creamy",
      "oxygen",
      "stage",
    ]);
    for (const preset of PORTRAIT_PRESETS) {
      for (const key of ["smooth", "texture", "evenSkin", "brighten"] as const) {
        expect(preset.settings[key]).toBeGreaterThanOrEqual(0);
        expect(preset.settings[key]).toBeLessThanOrEqual(100);
      }
      expect(preset.settings.texture).toBeGreaterThan(0);
    }
  });

  it("exposes color presets with the original first", () => {
    expect(COLOR_PRESETS[0]?.id).toBe("original");
    expect(isDefaultColor(COLOR_PRESETS[0]!.settings)).toBe(true);
    expect(COLOR_PRESETS).toHaveLength(4);
  });

  it("lists only effects whose value is above zero", () => {
    const enabled = enabledPortraitEffects({
      ...DEFAULT_PORTRAIT,
      smooth: 22,
      brighten: 8,
    });
    expect(enabled.map((effect) => effect.key)).toEqual(["smooth", "brighten"]);
    expect(enabledPortraitEffects(DEFAULT_PORTRAIT)).toHaveLength(0);
    expect(BASE_PORTRAIT_EFFECTS).toHaveLength(3);
    expect(BEAUTY_PORTRAIT_EFFECTS).toHaveLength(3);
    expect(FINE_PORTRAIT_EFFECTS).toHaveLength(9);
    expect(WRINKLE_ZONE_EFFECTS).toHaveLength(7);
    expect(BODY_PORTRAIT_EFFECTS).toHaveLength(6);
    expect(PORTRAIT_EFFECTS).toHaveLength(28);
  });
});

describe("shortcuts", () => {
  const base = { key: "", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false };

  it("resolves undo and redo on Windows", () => {
    expect(resolveModifierShortcut({ ...base, key: "z", ctrlKey: true })).toBe("undo");
    expect(resolveModifierShortcut({ ...base, key: "Z", ctrlKey: true, shiftKey: true })).toBe("redo");
    expect(resolveModifierShortcut({ ...base, key: "y", ctrlKey: true })).toBe("redo");
  });

  it("supports the meta key for macOS", () => {
    expect(resolveModifierShortcut({ ...base, key: "z", metaKey: true })).toBe("undo");
    expect(resolveModifierShortcut({ ...base, key: "z", metaKey: true, shiftKey: true })).toBe("redo");
  });

  it("ignores modifier-less or unrelated key presses", () => {
    expect(resolveModifierShortcut({ ...base, key: "z" })).toBeNull();
    expect(resolveModifierShortcut({ ...base, key: "x", ctrlKey: true })).toBeNull();
    expect(resolveModifierShortcut({ ...base, key: "z", ctrlKey: true, altKey: true })).toBeNull();
    expect(resolveModifierShortcut({ ...base, key: "y", ctrlKey: true, shiftKey: true })).toBeNull();
  });
});
