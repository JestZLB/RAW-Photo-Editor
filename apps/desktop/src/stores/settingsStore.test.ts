// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_SETTINGS,
  SETTINGS_STORAGE_KEY,
  sanitizeSettings,
  useSettingsStore,
} from "./settingsStore";

describe("settings store", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useSettingsStore.setState({ ...DEFAULT_SETTINGS });
  });

  it("falls back to defaults for unknown or malformed values", () => {
    expect(sanitizeSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(
      sanitizeSettings({
        previewMaxDimension: 9000,
        exportFormat: "bmp",
        exportQuality: Number.NaN,
        exportSize: "9999",
        portraitIntensity: 12,
        reduceMotion: "yes",
      }),
    ).toEqual(DEFAULT_SETTINGS);
  });

  it("clamps quality and accepts known choices", () => {
    const high = sanitizeSettings({ ...DEFAULT_SETTINGS, exportQuality: 320 });
    expect(high.exportQuality).toBe(100);

    const low = sanitizeSettings({ ...DEFAULT_SETTINGS, exportQuality: -4 });
    expect(low.exportQuality).toBe(1);

    const chosen = sanitizeSettings({
      previewMaxDimension: 2048,
      exportFormat: "png",
      exportQuality: 83,
      exportSize: "16:9",
      exportCustomWidth: 1600,
      exportCustomHeight: 900,
      portraitIntensity: 55,
      theme: "ink",
      reduceMotion: true,
      exportColorSpace: "displayP3",
      exportMetadata: "privacy",
      exportBitDepth: "sixteen",
    });
    expect(chosen).toEqual({
      previewMaxDimension: 2048,
      exportFormat: "png",
      exportQuality: 83,
      exportSize: "16:9",
      exportCustomWidth: 1600,
      exportCustomHeight: 900,
      portraitIntensity: 55,
      theme: "ink",
      reduceMotion: true,
      exportColorSpace: "displayP3",
      exportMetadata: "privacy",
      exportBitDepth: "sixteen",
      aiDenoiseModelPath: null,
      segmentationModelPath: null,
      aiDenoiseStrength: 100,
      aiDenoiseEngine: "photoHq",
      photoHqStrength: 80,
      watermarkEnabled: false,
      watermarkKind: "text",
      watermarkText: "",
      watermarkImage: null,
      watermarkOpacity: 60,
      watermarkScale: 18,
      watermarkPosition: "bottomRight",
    });
    expect(sanitizeSettings({ ...DEFAULT_SETTINGS, theme: "neon" }).theme).toBe("ink");
    expect(sanitizeSettings({ ...DEFAULT_SETTINGS, theme: "sand" }).theme).toBe("sand");
    // An unknown colour space must not survive a reload.
    expect(
      sanitizeSettings({ ...DEFAULT_SETTINGS, exportColorSpace: "rec2020" }).exportColorSpace,
    ).toBe("srgb");
    expect(
      sanitizeSettings({ ...DEFAULT_SETTINGS, exportMetadata: "blur", exportBitDepth: 12 })
        .exportMetadata,
    ).toBe("keep");
    expect(sanitizeSettings({ ...DEFAULT_SETTINGS, exportBitDepth: 12 }).exportBitDepth).toBe(
      "eight",
    );
    expect(
      sanitizeSettings({ ...DEFAULT_SETTINGS, aiDenoiseModelPath: "  ", aiDenoiseStrength: 900 }),
    ).toMatchObject({ aiDenoiseModelPath: null, aiDenoiseStrength: 100 });
    expect(
      sanitizeSettings({ ...DEFAULT_SETTINGS, segmentationModelPath: "  " }),
    ).toMatchObject({ segmentationModelPath: null });
    expect(
      sanitizeSettings({ ...DEFAULT_SETTINGS, segmentationModelPath: " D:\\m\\seg.onnx " }),
    ).toMatchObject({ segmentationModelPath: "D:\\m\\seg.onnx" });
    expect(
      sanitizeSettings({
        ...DEFAULT_SETTINGS,
        aiDenoiseModelPath: " C:\\models\\denoise.onnx ",
        aiDenoiseStrength: 55.4,
      }),
    ).toMatchObject({
      aiDenoiseModelPath: "C:\\models\\denoise.onnx",
      aiDenoiseStrength: 55,
    });
    expect(
      sanitizeSettings({
        ...DEFAULT_SETTINGS,
        aiDenoiseEngine: "python",
        photoHqStrength: 180,
      }),
    ).toMatchObject({ aiDenoiseEngine: "photoHq", photoHqStrength: 100 });
  });

  it("sanitizes watermark content and ranges", () => {
    const text = sanitizeSettings({
      ...DEFAULT_SETTINGS,
      watermarkEnabled: true,
      watermarkText: "  Luma Studio  ",
      watermarkOpacity: 0,
      watermarkScale: 99,
      watermarkPosition: "middle",
    });
    expect(text).toMatchObject({
      watermarkEnabled: true,
      watermarkText: "Luma Studio",
      watermarkOpacity: 5,
      watermarkScale: 60,
      watermarkPosition: "bottomRight",
    });

    const remote = sanitizeSettings({
      ...DEFAULT_SETTINGS,
      watermarkImage: "https://example.com/logo.png",
    });
    expect(remote.watermarkImage).toBeNull();

    const dataUrl = "data:image/png;base64,AAAA";
    expect(
      sanitizeSettings({ ...DEFAULT_SETTINGS, watermarkImage: dataUrl }).watermarkImage,
    ).toBe(dataUrl);
  });

  it("persists updates and resets back to defaults", () => {
    useSettingsStore.getState().update({
      previewMaxDimension: 1200,
      exportFormat: "png",
      exportQuality: 77,
      exportSize: "custom",
      exportCustomWidth: 1600,
      exportCustomHeight: 2000,
      portraitIntensity: 100,
      reduceMotion: true,
    });

    expect(useSettingsStore.getState().previewMaxDimension).toBe(1200);
    expect(useSettingsStore.getState().portraitIntensity).toBe(100);
    expect(useSettingsStore.getState().exportCustomWidth).toBe(1600);
    const stored = JSON.parse(window.localStorage.getItem(SETTINGS_STORAGE_KEY) ?? "{}");
    expect(stored.exportFormat).toBe("png");
    expect(stored.exportQuality).toBe(77);
    expect(stored.exportSize).toBe("custom");

    useSettingsStore.getState().reset();
    expect(useSettingsStore.getState().previewMaxDimension).toBe(
      DEFAULT_SETTINGS.previewMaxDimension,
    );
    expect(useSettingsStore.getState().reduceMotion).toBe(false);
    const reset = JSON.parse(window.localStorage.getItem(SETTINGS_STORAGE_KEY) ?? "{}");
    expect(reset).toEqual(DEFAULT_SETTINGS);
  });
});
