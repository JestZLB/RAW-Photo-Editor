import { describe, expect, it } from "vitest";
import { neutralWhiteBalance, type WhiteBalanceSpace } from "./whiteBalance";

function applyGains(
  srgb: [number, number, number],
  temperature: number,
  tint: number,
  space: WhiteBalanceSpace,
): [number, number, number] {
  const transform = space === "linear" ? (value: number) => srgbToLinear(value) : (value: number) => value;
  const inverse = space === "linear" ? (value: number) => linearToSrgb(value) : (value: number) => value;
  const t = temperature / 100;
  const u = tint / 100;
  const gains: [number, number, number] = [
    (1 + 0.25 * t) * (1 + 0.05 * u),
    1 - 0.15 * u,
    (1 - 0.25 * t) * (1 + 0.05 * u),
  ];
  const linear = srgb.map(transform) as [number, number, number];
  return linear.map((value, index) => inverse(value * gains[index])) as [number, number, number];
}

function srgbToLinear(value: number): number {
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(value: number): number {
  return value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
}

function spread(channels: [number, number, number]): number {
  return Math.max(...channels) - Math.min(...channels);
}

describe("neutralWhiteBalance", () => {
  it.each([
    [[180, 170, 160], "linear"],
    [[140, 150, 165], "linear"],
    [[180, 170, 160], "display"],
    [[140, 150, 165], "display"],
  ] as const)("neutralizes sample %j in %s space", (bytes, space) => {
    const sample = bytes.map((value) => value / 255) as [number, number, number];
    const suggestion = neutralWhiteBalance(sample, space);
    expect(suggestion).not.toBeNull();
    const corrected = applyGains(sample, suggestion!.temperature, suggestion!.tint, space);
    expect(spread(corrected)).toBeLessThan(0.02);
  });

  it("rejects black, clipped and non-finite samples", () => {
    expect(neutralWhiteBalance([0, 0, 0])).toBeNull();
    expect(neutralWhiteBalance([0.01, 0.01, 0.015])).toBeNull();
    expect(neutralWhiteBalance([1, 1, 1])).toBeNull();
    expect(neutralWhiteBalance([0.7, Number.NaN, 0.6])).toBeNull();
  });
});
