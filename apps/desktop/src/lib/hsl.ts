function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lightness = (max + min) / 2;
  const delta = max - min;
  if (delta <= 1e-6) return [0, 0, lightness];
  const saturation = delta / Math.max(1e-6, 1 - Math.abs(2 * lightness - 1));
  const hue =
    (max === r
      ? ((g - b) / delta + 6) % 6
      : max === g
        ? (b - r) / delta + 2
        : (r - g) / delta + 4) / 6;
  return [hue, Math.min(1, Math.max(0, saturation)), lightness];
}

function hslToRgb(hue: number, saturation: number, lightness: number): [number, number, number] {
  const h = ((hue % 1) + 1) % 1;
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const x = chroma * (1 - Math.abs(((h * 6) % 2) - 1));
  const sector = Math.floor(h * 6);
  const [r, g, b] =
    sector === 0
      ? [chroma, x, 0]
      : sector === 1
        ? [x, chroma, 0]
        : sector === 2
          ? [0, chroma, x]
          : sector === 3
            ? [0, x, chroma]
            : sector === 4
              ? [x, 0, chroma]
              : [chroma, 0, x];
  const m = lightness - chroma / 2;
  return [
    Math.min(1, Math.max(0, r + m)),
    Math.min(1, Math.max(0, g + m)),
    Math.min(1, Math.max(0, b + m)),
  ];
}

/** Apply a display-space HSL shift, mirroring the Rust color pipeline. */
export function applyHsl(
  r: number,
  g: number,
  b: number,
  hue: number,
  saturation: number,
  lightness: number,
): [number, number, number] {
  if (hue === 0 && saturation === 0 && lightness === 0) return [r, g, b];
  const [currentHue, currentSaturation, currentLightness] = rgbToHsl(r, g, b);
  const nextHue = (((currentHue + hue / 360) % 1) + 1) % 1;
  const nextSaturation = Math.min(1, Math.max(0, currentSaturation * (1 + saturation / 100)));
  const amount = (lightness / 100) * 0.35;
  const nextLightness =
    amount >= 0
      ? currentLightness + (1 - currentLightness) * amount
      : currentLightness * (1 + amount);
  return hslToRgb(nextHue, nextSaturation, Math.min(1, Math.max(0, nextLightness)));
}
