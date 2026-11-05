import { describe, expect, it } from "vitest";
import {
  aspectRatioOf,
  clampCropToBounds,
  clampNormalizedCrop,
  cropBounds,
  draftToRotatedPixels,
  fitCropForAngle,
  normalizedCropToPixels,
  rotatedFrameSize,
  type NormalizedCrop,
} from "./crop";

function expectInsideRotatedPhoto(
  bounds: NormalizedCrop,
  width: number,
  height: number,
  angle: number,
): void {
  const frame = rotatedFrameSize(width, height, angle);
  const rect = draftToRotatedPixels(bounds, frame.width, frame.height);
  const radians = (Math.abs(angle) * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const corners = [
    [rect.x, rect.y],
    [rect.x + rect.width, rect.y],
    [rect.x, rect.y + rect.height],
    [rect.x + rect.width, rect.y + rect.height],
  ];
  for (const [x, y] of corners) {
    const u = x - frame.width / 2;
    const v = y - frame.height / 2;
    expect(Math.abs(cos * u + sin * v)).toBeLessThanOrEqual(width / 2 + 1.5);
    expect(Math.abs(sin * u - cos * v)).toBeLessThanOrEqual(height / 2 + 1.5);
  }
}

describe("crop helpers", () => {
  it("centers a square crop inside a portrait frame", () => {
    const crop = cropBounds(768, 966, 0, 1);
    expect(crop.width).toBeCloseTo(1);
    expect(crop.height).toBeCloseTo(768 / 966);
    expect(crop.x).toBeCloseTo(0);
    expect(crop.y).toBeCloseTo((1 - 768 / 966) / 2);
  });

  it("shrinks the safe crop as the photo rotates", () => {
    const flat = cropBounds(1000, 1000, 0, null);
    expect(flat).toEqual({ x: 0, y: 0, width: 1, height: 1 });
    const turned = cropBounds(1000, 1000, 45, null);
    expect(turned.width).toBeCloseTo(0.5);
    expect(turned.height).toBeCloseTo(0.5);
    expect(turned.x).toBeCloseTo(0.25);
    expect(turned.y).toBeCloseTo(0.25);
  });

  it("keeps rotated crops inside the photo and never upscales", () => {
    const bounds = cropBounds(4000, 3000, 20, null);
    const frame = rotatedFrameSize(4000, 3000, 20);
    expect(bounds.width * frame.width).toBeLessThanOrEqual(4000);
    expect(bounds.height * frame.height).toBeLessThanOrEqual(3000);
    expectInsideRotatedPhoto(bounds, 4000, 3000, 20);
  });

  it("fits fixed ratios inside the rotated photo", () => {
    const bounds = cropBounds(4000, 3000, 20, 16 / 9);
    const frame = rotatedFrameSize(4000, 3000, 20);
    expect((bounds.width * frame.width) / (bounds.height * frame.height)).toBeCloseTo(16 / 9);
    expectInsideRotatedPhoto(bounds, 4000, 3000, 20);
  });

  it("auto-shrinks the draft around its center when rotating", () => {
    const full = { x: 0, y: 0, width: 1, height: 1 };
    const next = fitCropForAngle(full, 0, 45, 1000, 1000, null);
    expect(next.width).toBeCloseTo(0.5);
    expect(next.height).toBeCloseTo(0.5);
    expect(next.x).toBeCloseTo(0.25);
    expect(next.y).toBeCloseTo(0.25);
  });

  it("clamps drags into the safe area without distorting a fixed ratio", () => {
    const bounds = cropBounds(4000, 3000, 20, 1);
    const clamped = clampCropToBounds({ x: 0.9, y: -0.2, width: 0.8, height: 0.8 }, bounds, true);
    expect(clamped.width / clamped.height).toBeCloseTo(1);
    expect(clamped.x).toBeGreaterThanOrEqual(bounds.x - 1e-9);
    expect(clamped.y).toBeGreaterThanOrEqual(bounds.y - 1e-9);
    expect(clamped.x + clamped.width).toBeLessThanOrEqual(bounds.x + bounds.width + 1e-9);
    expect(clamped.y + clamped.height).toBeLessThanOrEqual(bounds.y + bounds.height + 1e-9);
  });

  it("maps a normalized draft onto the current absolute crop", () => {
    const absolute = normalizedCropToPixels(
      { x: 0.5, y: 0.5, width: 0.5, height: 0.5 },
      { x: 100, y: 50, width: 400, height: 300 },
    );
    expect(absolute).toEqual({ x: 300, y: 200, width: 200, height: 150 });
  });

  it("clamps drafts inside the image and rejects unknown ratios", () => {
    const clamped = clampNormalizedCrop({ x: -1, y: 2, width: 2, height: 0 });
    expect(clamped).toEqual({ x: 0, y: 0.98, width: 1, height: 0.02 });
    expect(aspectRatioOf("free")).toBeNull();
    expect(aspectRatioOf("4:5")).toBeCloseTo(0.8);
  });

  it("computes the rotated bounding box and maps a draft into it", () => {
    expect(rotatedFrameSize(16, 16, 45)).toEqual({ width: 23, height: 23 });
    expect(rotatedFrameSize(4000, 3000, 0)).toEqual({ width: 4000, height: 3000 });
    expect(rotatedFrameSize(4000, 3000, 90)).toEqual({ width: 3000, height: 4000 });
    expect(draftToRotatedPixels({ x: 0.1, y: 0.2, width: 0.5, height: 0.25 }, 1000, 800)).toEqual({
      x: 100,
      y: 160,
      width: 500,
      height: 200,
    });
  });
});
