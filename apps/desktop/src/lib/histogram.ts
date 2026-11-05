import type { AutoToneResult, Histogram } from "../api/types";

const PREVIEW_BINS = 64;

function lumaAt(data: Uint8ClampedArray, index: number): number {
  return Math.floor(
    (2126 * data[index] + 7152 * data[index + 1] + 722 * data[index + 2]) / 10000,
  );
}

export function histogramFromRgba(data: Uint8ClampedArray): Histogram {
  const red = new Array<number>(PREVIEW_BINS).fill(0);
  const green = new Array<number>(PREVIEW_BINS).fill(0);
  const blue = new Array<number>(PREVIEW_BINS).fill(0);
  const luma = new Array<number>(PREVIEW_BINS).fill(0);
  for (let index = 0; index < data.length; index += 4) {
    red[Math.floor((data[index] * PREVIEW_BINS) / 256)] += 1;
    green[Math.floor((data[index + 1] * PREVIEW_BINS) / 256)] += 1;
    blue[Math.floor((data[index + 2] * PREVIEW_BINS) / 256)] += 1;
    luma[Math.floor((lumaAt(data, index) * PREVIEW_BINS) / 256)] += 1;
  }
  return { red, green, blue, luma };
}

export function lumaHistogramFromRgba(data: Uint8ClampedArray): number[] {
  const histogram = new Array<number>(256).fill(0);
  for (let index = 0; index < data.length; index += 4) {
    histogram[Math.min(255, lumaAt(data, index))] += 1;
  }
  return histogram;
}

/**
 * Mirror of the Rust `auto_tone_from_luma_histogram` helper. Kept in display
 * space so the demo and native app suggest the same conservative corrections.
 */
export function autoToneFromLumaHistogram(
  histogram: readonly number[],
): AutoToneResult | null {
  const total = histogram.reduce((sum, value) => sum + value, 0);
  if (total <= 0 || histogram.length < 2) return null;
  const last = histogram.length - 1;

  const percentile = (quantile: number): number => {
    const target = Math.max(1, Math.ceil(total * quantile));
    let cumulative = 0;
    for (let index = 0; index < histogram.length; index += 1) {
      cumulative += histogram[index];
      if (cumulative >= target) return index / last;
    }
    return 1;
  };

  const shadows = percentile(0.02);
  const median = Math.max(0.04, percentile(0.5));
  const highlights = percentile(0.98);
  const medianExposure = Math.log2(0.42 / median);
  const highlightRoom = Math.max(0, Math.log2(0.96 / Math.max(0.05, highlights)));
  const exposure = Math.min(
    1,
    Math.max(-1, medianExposure > 0 ? Math.min(medianExposure, highlightRoom) : medianExposure),
  );
  const spread = highlights - shadows;
  const contrast =
    spread < 0.42
      ? Math.min(15, ((0.42 - spread) / 0.42) * 15)
      : spread > 0.9
        ? Math.max(-10, ((0.9 - spread) / 0.4) * 10)
        : 0;
  const blacks = Math.min(4, Math.max(-8, (0.04 - shadows) * 80));
  const whites = Math.min(4, Math.max(-8, (0.94 - highlights) * 60));

  return {
    exposure: Math.round(exposure * 100) / 100,
    contrast: Math.round(contrast * 10) / 10,
    whites: Math.round(whites * 10) / 10,
    blacks: Math.round(blacks * 10) / 10,
  };
}

/**
 * Scale histograms by a high percentile instead of the absolute peak: a large
 * flat background produces one spike that would flatten everything else.
 */
export function histogramScale(values: number[], percentile = 0.985): number {
  if (values.length === 0) return 1;
  const sorted = values.slice().sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.floor(sorted.length * percentile));
  return Math.max(1, sorted[index] ?? 1);
}
