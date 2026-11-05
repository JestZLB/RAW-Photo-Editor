export function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

/**
 * Monotone cubic interpolation (Fritsch–Carlson) through evenly spaced values.
 *
 * A tone curve must not overshoot between handles: overshoot would make a
 * brighter input map to a darker output and invert local contrast.
 */
export function sampleMonotoneCurve(
  values: number[],
  samples = 96,
): Array<{ x: number; y: number }> {
  const count = Math.max(1, Math.round(samples));
  if (values.length === 0) return [];
  if (values.length === 1) {
    return [{ x: 0, y: clamp01(values[0]) }];
  }
  const n = values.length;
  const step = 1 / (n - 1);
  const deltas = values.slice(0, -1).map((value, index) => (values[index + 1] - value) / step);
  const tangents = new Array<number>(n);
  tangents[0] = deltas[0];
  tangents[n - 1] = deltas[n - 2];
  for (let index = 1; index < n - 1; index += 1) {
    tangents[index] = (deltas[index - 1] + deltas[index]) / 2;
  }
  for (let index = 0; index < n - 1; index += 1) {
    const delta = deltas[index];
    if (Math.abs(delta) < 1e-9) {
      tangents[index] = 0;
      tangents[index + 1] = 0;
      continue;
    }
    const left = tangents[index] / delta;
    const right = tangents[index + 1] / delta;
    const magnitude = left * left + right * right;
    if (magnitude > 9) {
      const scale = 3 / Math.sqrt(magnitude);
      tangents[index] = scale * left * delta;
      tangents[index + 1] = scale * right * delta;
    }
  }

  const out: Array<{ x: number; y: number }> = [];
  for (let sample = 0; sample <= count; sample += 1) {
    const x = sample / count;
    const scaled = x / step;
    const index = Math.min(n - 2, Math.floor(scaled));
    const t = scaled - index;
    const t2 = t * t;
    const t3 = t2 * t;
    const h00 = 2 * t3 - 3 * t2 + 1;
    const h10 = t3 - 2 * t2 + t;
    const h01 = -2 * t3 + 3 * t2;
    const h11 = t3 - t2;
    const y =
      h00 * values[index] +
      h10 * step * tangents[index] +
      h01 * values[index + 1] +
      h11 * step * tangents[index + 1];
    out.push({ x, y: clamp01(y) });
  }
  return out;
}
