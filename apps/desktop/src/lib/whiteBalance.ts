export interface WhiteBalanceSuggestion {
  temperature: number;
  tint: number;
}

export type WhiteBalanceSpace = "linear" | "display";

function srgbToLinear(value: number): number {
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function roundTenth(value: number): number {
  return Math.round(value * 10) / 10;
}

/**
 * Mirror of the Rust `color_pipeline::neutral_white_balance` helper. The
 * canonical pipeline solves in linear light; the demo approximates the same
 * gains in display space, so it passes `space: "display"`.
 */
export function neutralWhiteBalance(
  srgb: readonly [number, number, number],
  space: WhiteBalanceSpace = "linear",
): WhiteBalanceSuggestion | null {
  if (!srgb.every((value) => Number.isFinite(value))) return null;
  const display = srgb.map((value) => clamp(value, 0, 1)) as [number, number, number];
  if (display.some((value) => value >= 0.995)) return null;

  const channels = (space === "linear" ? display.map(srgbToLinear) : display) as [
    number,
    number,
    number,
  ];
  const [r, g, b] = channels;
  const luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  if (luma < (space === "linear" ? 0.01 : 0.08) || r < 1e-4 || b < 1e-4) {
    return null;
  }

  const ratio = r / b;
  const temperature = clamp((4 * (1 - ratio)) / (1 + ratio) * 100, -100, 100);

  const balancedRed = r * (1 + 0.25 * (temperature / 100));
  const denominator = 0.15 * g + 0.05 * balancedRed;
  const tint =
    Math.abs(denominator) < 1e-6
      ? 0
      : clamp(((g - balancedRed) / denominator) * 100, -100, 100);

  return { temperature: roundTenth(temperature), tint: roundTenth(tint) };
}
