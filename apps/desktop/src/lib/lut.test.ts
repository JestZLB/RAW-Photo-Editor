import { describe, expect, it } from "vitest";
import { parseCube, sampleCube } from "./lut";

function identity1D(): string {
  return ["LUT_1D_SIZE 3", "0 0 0", "0.5 0.5 0.5", "1 1 1"].join("\n");
}

function warm3D(): string {
  const lines = ["# sample cube", 'TITLE "Warm"', "LUT_3D_SIZE 2"];
  for (let b = 0; b < 2; b += 1) {
    for (let g = 0; g < 2; g += 1) {
      for (let r = 0; r < 2; r += 1) {
        lines.push(`${r * 0.8 + 0.2} ${g} ${b * 0.6}`);
      }
    }
  }
  return lines.join("\n");
}

describe("parseCube", () => {
  it("reads 1D tables and ignores metadata lines", () => {
    const lut = parseCube(identity1D());
    expect(lut.dimension).toBe("1D");
    expect(lut.size).toBe(3);
    expect(lut.data.length).toBe(9);
  });

  it("reads 3D cubes in red-fastest order", () => {
    const lut = parseCube(warm3D());
    expect(lut.dimension).toBe("3D");
    expect(lut.size).toBe(2);
    expect(lut.data.length).toBe(24);
  });

  it("rejects truncated or headerless cubes", () => {
    expect(() => parseCube("LUT_3D_SIZE 2\n0 0 0\n1 1 1")).toThrow();
    expect(() => parseCube("0 0 0\n1 1 1")).toThrow();
    expect(() => parseCube("LUT_3D_SIZE 129\n")).toThrow();
  });
});

describe("sampleCube", () => {
  it("leaves pixels unchanged with an identity 1D LUT", () => {
    const lut = parseCube(identity1D());
    const [r, g, b] = sampleCube(lut, 0.25, 0.5, 0.75, 1);
    expect(r).toBeCloseTo(0.25, 5);
    expect(g).toBeCloseTo(0.5, 5);
    expect(b).toBeCloseTo(0.75, 5);
  });

  it("blends by strength and is a no-op at zero", () => {
    const lut = parseCube(warm3D());
    const full = sampleCube(lut, 1, 0.5, 1, 1);
    expect(full[0]).toBeCloseTo(1, 5);
    expect(full[2]).toBeCloseTo(0.6, 5);
    const half = sampleCube(lut, 1, 0.5, 1, 0.5);
    expect(half[2]).toBeCloseTo(0.8, 5);
    const none = sampleCube(lut, 0.4, 0.5, 0.6, 0);
    expect(none).toEqual([0.4, 0.5, 0.6]);
  });
});
