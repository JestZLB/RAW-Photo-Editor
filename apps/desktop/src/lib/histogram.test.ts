import { describe, expect, it } from "vitest";
import {
  autoToneFromLumaHistogram,
  histogramFromRgba,
  lumaHistogramFromRgba,
} from "./histogram";

function pixels(...colors: Array<[number, number, number]>): Uint8ClampedArray {
  const data = new Uint8ClampedArray(colors.length * 4);
  colors.forEach((color, index) => {
    data[index * 4] = color[0];
    data[index * 4 + 1] = color[1];
    data[index * 4 + 2] = color[2];
    data[index * 4 + 3] = 255;
  });
  return data;
}

describe("histogram helpers", () => {
  it("counts every channel into 64 bins", () => {
    const histogram = histogramFromRgba(
      pixels([0, 0, 0], [255, 128, 64], [255, 255, 255]),
    );
    expect(histogram.luma).toHaveLength(64);
    expect(histogram.red.reduce((a, b) => a + b, 0)).toBe(3);
    expect(histogram.red[0]).toBe(1);
    expect(histogram.red[63]).toBe(2);
    expect(histogram.green[32]).toBe(1);
  });

  it("builds 256-bin luma data for auto tone", () => {
    const luma = lumaHistogramFromRgba(pixels([255, 255, 255], [0, 0, 0]));
    expect(luma).toHaveLength(256);
    expect(luma[255]).toBe(1);
    expect(luma[0]).toBe(1);
  });
});

describe("autoToneFromLumaHistogram", () => {
  it("brightens a dark frame and tones down a clipped one", () => {
    const dark = new Array(256).fill(0);
    dark[51] = 10_000;
    const darkSuggestion = autoToneFromLumaHistogram(dark)!;
    expect(darkSuggestion.exposure).toBeGreaterThan(0.8);

    const bright = new Array(256).fill(0);
    bright[255] = 10_000;
    const brightSuggestion = autoToneFromLumaHistogram(bright)!;
    expect(brightSuggestion.exposure).toBeLessThan(-0.5);
    expect(brightSuggestion.whites).toBeLessThan(0);
  });

  it("keeps every suggestion bounded", () => {
    const histogram = new Array(256).fill(0);
    histogram[0] = 2000;
    histogram[255] = 2000;
    for (let index = 1; index < 255; index += 1) histogram[index] = 100;
    const suggestion = autoToneFromLumaHistogram(histogram)!;
    expect(suggestion.exposure).toBeGreaterThanOrEqual(-1);
    expect(suggestion.exposure).toBeLessThanOrEqual(1);
    expect(suggestion.contrast).toBeGreaterThanOrEqual(-10);
    expect(suggestion.contrast).toBeLessThanOrEqual(15);
    expect(suggestion.whites).toBeGreaterThanOrEqual(-8);
    expect(suggestion.whites).toBeLessThanOrEqual(4);
    expect(suggestion.blacks).toBeGreaterThanOrEqual(-8);
    expect(suggestion.blacks).toBeLessThanOrEqual(4);
  });

  it("returns null for empty histograms", () => {
    expect(autoToneFromLumaHistogram([])).toBeNull();
    expect(autoToneFromLumaHistogram(new Array(256).fill(0))).toBeNull();
  });
});
