import { describe, expect, it } from "vitest";
import { outputDimensions } from "./export";

describe("export dimensions", () => {
  it("keeps the current crop for original size", () => {
    expect(outputDimensions(768, 966, "original")).toEqual({ width: 768, height: 966 });
  });

  it("computes native centered crops for ratio presets", () => {
    expect(outputDimensions(768, 966, "1:1")).toEqual({ width: 768, height: 768 });
    expect(outputDimensions(768, 966, "4:5")).toEqual({ width: 768, height: 960 });
    expect(outputDimensions(768, 966, "16:9")).toEqual({ width: 768, height: 432 });
  });

  it("uses custom dimensions and guards empty input", () => {
    expect(outputDimensions(768, 966, "custom", 900, 1200)).toEqual({
      width: 900,
      height: 1200,
    });
    expect(outputDimensions(0, 0, "original")).toEqual({ width: 1, height: 1 });
  });
});
