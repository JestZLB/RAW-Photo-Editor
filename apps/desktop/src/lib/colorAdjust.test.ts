import { describe, expect, it } from "vitest";
import { DEFAULT_COLOR } from "./params";
import { applyToneLut, buildToneLut, usesToneCurve } from "./colorAdjust";

describe("tone curve and levels", () => {
  it("identity parameters do not report an active curve", () => {
    expect(usesToneCurve(DEFAULT_COLOR)).toBe(false);
    const lut = buildToneLut(DEFAULT_COLOR);
    expect(applyToneLut(0.5, lut)).toBeCloseTo(0.5, 2);
  });

  it("raises midtones and clips shadows", () => {
    const curve = { ...DEFAULT_COLOR, curveY: [0, 0.25, 0.8, 0.9, 1] };
    expect(usesToneCurve(curve)).toBe(true);
    expect(applyToneLut(0.5, buildToneLut(curve))).toBeGreaterThan(0.7);

    const levels = { ...DEFAULT_COLOR, levelsInBlack: 0.2 };
    expect(applyToneLut(0.15, buildToneLut(levels))).toBeCloseTo(0, 5);
  });
});
