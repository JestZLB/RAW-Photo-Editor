import { describe, expect, it } from "vitest";
import {
  brushRadiusInFrame,
  defaultMaskGradient,
  maskSummary,
  normalizeMask,
  sourcePointToView,
  viewPointToSource,
} from "./mask";

describe("mask coordinate mapping", () => {
  const crop = { x: 100, y: 50, width: 400, height: 300 };

  it("maps view points to original-source coordinates and back", () => {
    const source = viewPointToSource({ x: 0.5, y: 0.25 }, crop, 800, 600);
    expect(source.x).toBeCloseTo(0.375);
    expect(source.y).toBeCloseTo(0.208333);
    const view = sourcePointToView(source, crop, 800, 600);
    expect(view.x).toBeCloseTo(0.5);
    expect(view.y).toBeCloseTo(0.25);
  });

  it("scales brush radius from source to the visible frame", () => {
    expect(brushRadiusInFrame(0.05, crop, 800, 600, 400)).toBeCloseTo(30);
  });
});

describe("mask defaults and summary", () => {
  it("fills legacy masks with neutral options", () => {
    const mask = normalizeMask({
      strokes: [],
      autoSkin: true,
    });
    expect(mask.gradient).toBeNull();
    expect(mask.inverted).toBe(false);
    expect(mask.feather).toBe(0);
    expect(mask.density).toBe(1);
    expect(maskSummary(mask)).toBe("自动肤色");
  });

  it("describes gradient, inversion and density", () => {
    const gradient = defaultMaskGradient("radial");
    expect(gradient.kind).toBe("radial");
    expect(gradient.end.x).toBeGreaterThan(gradient.start.x);
    const summary = maskSummary({
      strokes: [],
      autoSkin: false,
      gradient,
      inverted: true,
      density: 0.5,
    });
    expect(summary).toContain("径向渐变");
    expect(summary).toContain("已反转");
    expect(summary).toContain("浓度 50%");
  });
});

describe("rotated crop mapping", () => {
  it("maps the source centre to the centre of a full rotated frame", () => {
    const crop = { x: 0, y: 0, width: 903, height: 1069, angle: 8.5 };
    const view = sourcePointToView({ x: 0.5, y: 0.5 }, crop, 768, 966, 8.5);
    expect(view.x).toBeCloseTo(0.5, 6);
    expect(view.y).toBeCloseTo(0.5, 6);
  });

  it("round-trips arbitrary points through the rotation", () => {
    const crop = { x: 40, y: 30, width: 600, height: 700, angle: -12.5 };
    for (const point of [
      { x: 0.2, y: 0.3 },
      { x: 0.5, y: 0.5 },
      { x: 0.81, y: 0.64 },
    ]) {
      const view = sourcePointToView(point, crop, 1000, 800, -12.5);
      const back = viewPointToSource(view, crop, 1000, 800, -12.5);
      expect(back.x).toBeCloseTo(point.x, 6);
      expect(back.y).toBeCloseTo(point.y, 6);
    }
  });

  it("keeps the old mapping when the angle is zero", () => {
    const crop = { x: 100, y: 50, width: 400, height: 300, angle: 0 };
    const view = sourcePointToView({ x: 0.5, y: 0.5 }, crop, 800, 600, 0);
    expect(view.x).toBeCloseTo(0.75, 6);
    expect(view.y).toBeCloseTo(250 / 300, 6);
  });
});
