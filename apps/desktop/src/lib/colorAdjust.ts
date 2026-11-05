import type { ColorParams } from "../api/types";

const IDENTITY_Y = [0, 0.25, 0.5, 0.75, 1];

function sampleCurve(points: number[], values: number[], input: number): number {
  const x = Math.min(1, Math.max(0, input));
  for (let index = 0; index < 4; index += 1) {
    if (x <= points[index + 1]) {
      const span = Math.max(1e-6, points[index + 1] - points[index]);
      const t = Math.min(1, Math.max(0, (x - points[index]) / span));
      return values[index] + (values[index + 1] - values[index]) * t;
    }
  }
  return values[4];
}

export function usesToneCurve(params: ColorParams): boolean {
  return (
    params.curveY.some((value, index) => Math.abs(value - IDENTITY_Y[index]) > 1e-5) ||
    Math.abs(params.levelsInBlack) > 1e-5 ||
    Math.abs(params.levelsInWhite - 1) > 1e-5 ||
    Math.abs(params.levelsGamma - 1) > 1e-5 ||
    Math.abs(params.levelsOutBlack) > 1e-5 ||
    Math.abs(params.levelsOutWhite - 1) > 1e-5
  );
}

export function buildToneLut(params: ColorParams): Float32Array {
  const lut = new Float32Array(256);
  const inputRange = Math.max(1e-6, params.levelsInWhite - params.levelsInBlack);
  const outputRange = params.levelsOutWhite - params.levelsOutBlack;
  for (let index = 0; index < 256; index += 1) {
    const normalized = Math.min(1, Math.max(0, (index / 255 - params.levelsInBlack) / inputRange));
    const gamma = Math.pow(normalized, params.levelsGamma);
    const leveled = params.levelsOutBlack + gamma * outputRange;
    lut[index] = Math.min(1, Math.max(0, sampleCurve(params.curveX, params.curveY, leveled)));
  }
  return lut;
}

export function applyToneLut(value: number, lut: Float32Array): number {
  const index = Math.round(Math.min(1, Math.max(0, value)) * 255);
  return lut[index];
}
