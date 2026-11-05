/**
 * `.cube` LUT parsing and sampling for the browser demo backend.
 * Mirrors `crates/color-pipeline/src/lut.rs` so the web preview and the Rust
 * renderer apply the same look.
 */

export interface CubeLut {
  dimension: "1D" | "3D";
  size: number;
  /** size*3 triples for 1D, size^3 triples for 3D, each channel 0..1. */
  data: Float32Array;
}

export class CubeParseError extends Error {}

function fail(message: string): never {
  throw new CubeParseError(message);
}

export function parseCube(text: string): CubeLut {
  let oneDSize: number | null = null;
  let threeDSize: number | null = null;
  const entries: number[] = [];

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.length === 0 || line.startsWith("#")) continue;
    const upper = line.toUpperCase();
    if (upper.startsWith("TITLE") || upper.startsWith("DOMAIN")) continue;
    if (upper.startsWith("LUT_1D_SIZE")) {
      const size = Number.parseInt(upper.slice("LUT_1D_SIZE".length).trim(), 10);
      if (!Number.isFinite(size) || size < 2 || size > 256) {
        fail("1D LUT size must be between 2 and 256");
      }
      oneDSize = size;
      continue;
    }
    if (upper.startsWith("LUT_3D_SIZE")) {
      const size = Number.parseInt(upper.slice("LUT_3D_SIZE".length).trim(), 10);
      if (!Number.isFinite(size) || size < 2 || size > 64) {
        fail("3D LUT size must be between 2 and 64");
      }
      threeDSize = size;
      continue;
    }
    const tokens = line.split(/\s+/);
    if (tokens.length < 3) continue;
    const values = tokens.slice(0, 3).map((token) => Number.parseFloat(token));
    if (values.some((value) => !Number.isFinite(value))) continue;
    entries.push(
      Math.min(1, Math.max(0, values[0])),
      Math.min(1, Math.max(0, values[1])),
      Math.min(1, Math.max(0, values[2])),
    );
  }

  if (oneDSize !== null) {
    if (entries.length !== oneDSize * 3) {
      fail(`1D LUT expects ${oneDSize} entries, found ${entries.length / 3}`);
    }
    return { dimension: "1D", size: oneDSize, data: Float32Array.from(entries) };
  }
  if (threeDSize !== null) {
    const expected = threeDSize ** 3;
    if (entries.length !== expected * 3) {
      fail(`3D LUT expects ${expected} entries, found ${entries.length / 3}`);
    }
    return { dimension: "3D", size: threeDSize, data: Float32Array.from(entries) };
  }
  fail("cube file has no LUT_1D_SIZE or LUT_3D_SIZE");
}

function sample1D(lut: CubeLut, channel: number, value: number): number {
  const maxIndex = lut.size - 1;
  const scaled = Math.min(1, Math.max(0, value)) * maxIndex;
  const lower = Math.floor(scaled);
  const upper = Math.min(lower + 1, maxIndex);
  const t = scaled - lower;
  const a = lut.data[lower * 3 + channel];
  const b = lut.data[upper * 3 + channel];
  return a + (b - a) * t;
}

function sample3D(lut: CubeLut, r: number, g: number, b: number): [number, number, number] {
  const maxIndex = lut.size - 1;
  const scaledR = Math.min(1, Math.max(0, r)) * maxIndex;
  const scaledG = Math.min(1, Math.max(0, g)) * maxIndex;
  const scaledB = Math.min(1, Math.max(0, b)) * maxIndex;
  const r0 = Math.floor(scaledR);
  const g0 = Math.floor(scaledG);
  const b0 = Math.floor(scaledB);
  const r1 = Math.min(r0 + 1, maxIndex);
  const g1 = Math.min(g0 + 1, maxIndex);
  const b1 = Math.min(b0 + 1, maxIndex);
  const fr = scaledR - r0;
  const fg = scaledG - g0;
  const fb = scaledB - b0;
  const at = (ri: number, gi: number, bi: number, channel: number) =>
    lut.data[(bi + lut.size * (gi + lut.size * ri)) * 3 + channel];

  const out: [number, number, number] = [0, 0, 0];
  for (let channel = 0; channel < 3; channel += 1) {
    const c000 = at(r0, g0, b0, channel);
    const c100 = at(r1, g0, b0, channel);
    const c010 = at(r0, g1, b0, channel);
    const c110 = at(r1, g1, b0, channel);
    const c001 = at(r0, g0, b1, channel);
    const c101 = at(r1, g0, b1, channel);
    const c011 = at(r0, g1, b1, channel);
    const c111 = at(r1, g1, b1, channel);
    const x00 = c000 + (c100 - c000) * fr;
    const x10 = c010 + (c110 - c010) * fr;
    const x01 = c001 + (c101 - c001) * fr;
    const x11 = c011 + (c111 - c011) * fr;
    const y0 = x00 + (x10 - x00) * fg;
    const y1 = x01 + (x11 - x01) * fg;
    out[channel] = y0 + (y1 - y0) * fb;
  }
  return out;
}

/** Maps a display-referred sRGB triplet through the LUT, blended by `strength`. */
export function sampleCube(
  lut: CubeLut,
  r: number,
  g: number,
  b: number,
  strength: number,
): [number, number, number] {
  if (!(strength > 0)) return [r, g, b];
  const amount = Math.min(1, Math.max(0, strength));
  const mapped =
    lut.dimension === "1D"
      ? [sample1D(lut, 0, r), sample1D(lut, 1, g), sample1D(lut, 2, b)]
      : sample3D(lut, r, g, b);
  return [
    r + (mapped[0] - r) * amount,
    g + (mapped[1] - g) * amount,
    b + (mapped[2] - b) * amount,
  ];
}
