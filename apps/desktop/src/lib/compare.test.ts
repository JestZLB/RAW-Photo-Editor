import { describe, expect, it } from "vitest";
import { sourcePlacement } from "./compare";

describe("split comparison placement", () => {
  it("maps the source image into a cropped preview", () => {
    const placement = sourcePlacement(
      1000,
      800,
      { x: 250, y: 100, width: 500, height: 400 },
      400,
      320,
    );
    expect(placement.width).toBeCloseTo(800);
    expect(placement.height).toBeCloseTo(640);
    expect(placement.left).toBeCloseTo(-200);
    expect(placement.top).toBeCloseTo(-80);
  });

  it("is a no-op for a full-frame crop", () => {
    const placement = sourcePlacement(800, 600, { x: 0, y: 0, width: 800, height: 600 }, 400, 300);
    expect(placement.width).toBe(400);
    expect(placement.height).toBe(300);
    expect(placement.left).toBeCloseTo(0);
    expect(placement.top).toBeCloseTo(0);
  });
});
