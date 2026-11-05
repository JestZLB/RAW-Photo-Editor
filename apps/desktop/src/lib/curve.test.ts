import { describe, expect, it } from "vitest";
import { clamp01, sampleMonotoneCurve } from "./curve";

describe("monotone tone curve", () => {
  it("keeps both endpoints pinned", () => {
    const curve = sampleMonotoneCurve([0.1, 0.4, 0.5, 0.8, 0.95], 32);
    expect(curve[0]).toEqual({ x: 0, y: 0.1 });
    expect(curve[curve.length - 1].x).toBe(1);
    expect(curve[curve.length - 1].y).toBeCloseTo(0.95, 6);
    expect(curve).toHaveLength(33);
  });

  it("reproduces the identity curve", () => {
    const curve = sampleMonotoneCurve([0, 0.25, 0.5, 0.75, 1], 16);
    for (const point of curve) {
      expect(point.y).toBeCloseTo(point.x, 5);
    }
  });

  it("never overshoots between handles", () => {
    const values = [0, 0.05, 0.9, 0.92, 1];
    const curve = sampleMonotoneCurve(values, 200);
    for (let index = 1; index < curve.length; index += 1) {
      expect(curve[index].y).toBeGreaterThanOrEqual(curve[index - 1].y - 1e-6);
    }
    for (const point of curve) {
      expect(point.y).toBeGreaterThanOrEqual(0);
      expect(point.y).toBeLessThanOrEqual(1);
    }
  });

  it("clamps out-of-range control values", () => {
    const curve = sampleMonotoneCurve([-1, 0.5, 2], 8);
    for (const point of curve) {
      expect(point.y).toBeGreaterThanOrEqual(0);
      expect(point.y).toBeLessThanOrEqual(1);
    }
    expect(clamp01(-0.4)).toBe(0);
    expect(clamp01(1.4)).toBe(1);
  });
});
