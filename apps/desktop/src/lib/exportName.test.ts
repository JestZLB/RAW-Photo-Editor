import { describe, expect, it } from "vitest";
import { DEFAULT_EXPORT_NAME_TEMPLATE, renderExportStem } from "./export";

// Local time on purpose: the tokens are user-facing, not UTC.
const AT = new Date(2026, 10, 6, 9, 5, 7);

describe("export filename template", () => {
  it("keeps the historical name-luma output for the default", () => {
    expect(DEFAULT_EXPORT_NAME_TEMPLATE).toBe("{name}-luma");
    expect(renderExportStem("", { name: "portrait" })).toBe("portrait-luma");
    expect(renderExportStem("   ", { name: "portrait" })).toBe("portrait-luma");
  });

  it("replaces every supported token", () => {
    const stem = renderExportStem(
      "{name}-{collection}-{date}-{time}-{index}-{width}x{height}",
      {
        name: "portrait",
        collection: "婚礼",
        width: 2048,
        height: 1365,
        index: 3,
        date: AT,
      },
    );
    expect(stem).toBe("portrait-婚礼-20261106-090507-03-2048x1365");
  });

  it("drops characters windows rejects", () => {
    expect(renderExportStem('{name}: "final"?', { name: "a/b" })).toBe("a-b-final");
  });

  it("leaves unknown tokens visible instead of silently dropping them", () => {
    // Keeping the braces makes the typo obvious in the export dialog preview.
    expect(renderExportStem("{nmae}-{name}", { name: "shot" })).toBe("{nmae}-shot");
  });

  it("falls back when nothing usable remains", () => {
    expect(renderExportStem("...", { name: "" })).toBe("luma-photo");
    expect(renderExportStem("{collection}", { name: "shot", collection: "" })).toBe(
      "luma-photo",
    );
  });

  it("caps the stem so the extension and a dedupe suffix still fit", () => {
    expect(renderExportStem("{name}", { name: "x".repeat(200) })).toHaveLength(96);
  });

  it("omits index/width/height when the caller has no value", () => {
    expect(renderExportStem("{name}-{index}{width}{height}", { name: "shot" })).toBe(
      "shot",
    );
  });
});
