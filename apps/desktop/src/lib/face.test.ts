import { describe, expect, it } from "vitest";
import {
  bestFaceIndex,
  faceLayerName,
  faceMaskRegion,
  faceQuality,
  faceScorePercent,
  hasFineEffects,
  portraitGeometryForFace,
} from "./face";
import type { PortraitFace, PortraitFaceRegions } from "../api/types";

describe("face helpers", () => {
  it("maps ensemble confidence directly to a percentage", () => {
    expect(faceScorePercent(0)).toBe(0);
    expect(faceScorePercent(0.5)).toBe(50);
    expect(faceScorePercent(0.87)).toBe(87);
    expect(faceScorePercent(1)).toBe(100);
    expect(faceQuality(0.5)).toBe("低");
    expect(faceQuality(0.65)).toBe("中");
    expect(faceQuality(0.9)).toBe("高");
  });

  it("builds a face oval when no measured skin region exists", () => {
    const face: PortraitFace = {
      faceId: "face-1",
      x: 400,
      y: 600,
      width: 200,
      height: 240,
      confidence: 0.9,
    };
    const region = faceMaskRegion(face, 1000, 2000);
    expect(region.kind).toBe("face");
    expect(region.centerX).toBeCloseTo(0.5);
    expect(region.centerY).toBeCloseTo(0.36);
    expect(region.radiusX).toBeCloseTo(0.104);
    expect(region.feather).toBeGreaterThan(0.5);
  });

  it("prefers the measured skin ellipse", () => {
    const face: PortraitFace = {
      faceId: "face-1",
      x: 0,
      y: 0,
      width: 10,
      height: 10,
      confidence: 0.9,
    };
    const measured = {
      faceId: "face-1",
      box: face,
      keypoints: [],
      score: 0.8,
      regions: [
        {
          kind: "skin",
          label: "脸部皮肤",
          centerX: 0.4,
          centerY: 0.3,
          radiusX: 0.08,
          radiusY: 0.11,
          angle: 0.2,
          feather: 0.6,
          confidence: 0.8,
        },
      ],
    } as PortraitFaceRegions;
    const region = faceMaskRegion(face, 1000, 1000, measured);
    expect(region.centerX).toBeCloseTo(0.4);
    expect(region.centerY).toBeCloseTo(0.3);
    expect(region.radiusY).toBeCloseTo(0.11);
  });

  it("names per-face layers clearly", () => {
    expect(faceLayerName(0, 1)).toBe("人像美颜");
    expect(faceLayerName(2, 3)).toBe("人脸 3 美颜");
  });

  it("selects the strongest detection without reordering the list", () => {
    expect(bestFaceIndex([])).toBe(0);
    expect(bestFaceIndex([{ confidence: 0.6 }, { confidence: 0.9 }])).toBe(1);
    expect(bestFaceIndex([{ confidence: 0.9 }, { confidence: 0.8 }])).toBe(0);
  });

  it("uses measured eyes and mouth for fine-repair geometry", () => {
    const face: PortraitFace = {
      faceId: "face-1",
      x: 400,
      y: 600,
      width: 200,
      height: 240,
      confidence: 0.82,
    };
    const measured = {
      faceId: "face-1",
      box: face,
      keypoints: [],
      score: 0.8,
      regions: [
        {
          kind: "skin",
          label: "脸部皮肤",
          centerX: 0.4,
          centerY: 0.3,
          radiusX: 0.08,
          radiusY: 0.11,
          angle: 0,
          feather: 0.5,
          confidence: 0.8,
        },
        {
          kind: "leftEye",
          label: "左眼",
          centerX: 0.36,
          centerY: 0.26,
          radiusX: 0.02,
          radiusY: 0.01,
          angle: 0,
          feather: 0.4,
          confidence: 0.75,
        },
      ],
    } as PortraitFaceRegions;
    const geometry = portraitGeometryForFace(face, 1000, 1000, measured);
    expect(geometry.face?.centerX).toBeCloseTo(0.4);
    expect(geometry.face?.radiusY).toBeCloseTo(0.11);
    expect(geometry.leftEye?.centerX).toBeCloseTo(0.36);
    expect(geometry.rightEye).toBeNull();
    expect(geometry.mouth).toBeNull();
  });

  it("falls back to the detector box when no region was measured", () => {
    const face: PortraitFace = {
      faceId: "face-1",
      x: 400,
      y: 600,
      width: 200,
      height: 240,
      confidence: 0.82,
    };
    const geometry = portraitGeometryForFace(face, 1000, 2000);
    expect(geometry.face?.centerX).toBeCloseTo(0.5);
    expect(geometry.face?.centerY).toBeCloseTo(0.36);
    expect(geometry.face?.radiusX).toBeCloseTo(0.104);
    expect(geometry.face?.radiusY).toBeCloseTo(0.0744);
    expect(geometry.leftEye).toBeNull();
  });

  it("detects whether any fine-repair effect is on", () => {
    const resting = {
      eyeEnhance: 0,
      teethWhiten: 0,
      blemish: 0,
      wrinkle: 0,
      darkCircle: 0,
      mole: 0,
      freckle: 0,
      birthmark: 0,
      tattoo: 0,
      whiten: 0,
      rosy: 0,
      eyeBag: 0,
      wrinkleForehead: 0,
      wrinkleGlabella: 0,
      wrinkleCrowFeet: 0,
      wrinkleNasolabial: 0,
      wrinkleMouthCorner: 0,
      wrinkleUnderEye: 0,
      wrinkleNeck: 0,
      faceSlim: 0,
      eyeSize: 0,
      noseSlim: 0,
      oilControl: 0,
      eyeWhite: 0,
      irisDetail: 0,
      teethBrighten: 0,
      lipTint: 0,
      lipWarmth: 0,
      noseContour: 0,
      eyeshadow: 0,
      browTint: 0,
      jawWidth: 0,
      chinLength: 0,
      eyeDistance: 0,
    };
    expect(hasFineEffects(resting)).toBe(false);
    expect(hasFineEffects({ ...resting, teethWhiten: 10 })).toBe(true);
    expect(hasFineEffects({ ...resting, whiten: 10 })).toBe(true);
    expect(hasFineEffects({ ...resting, wrinkleNeck: 10 })).toBe(true);
    expect(hasFineEffects({ ...resting, faceSlim: 10 })).toBe(true);
  });
});
