import { describe, expect, it } from "vitest";
// Vite inlines the file as text; `vite/client` types the `?raw` suffix.
import stylesheet from "../styles.css?raw";

/**
 * Guards the theme tokens in `styles.css`.
 *
 * A custom property that references itself is a cycle and becomes invalid at
 * computed-value time, so every consumer silently falls back to "unset"
 * (transparent background, no border). That is invisible to typecheck and to
 * component tests, and it already shipped once for the default theme, so it is
 * asserted here against the real stylesheet. Comments are stripped first: the
 * explanatory comment in the stylesheet quotes the broken form.
 */
const declarations = stylesheet.replace(/\/\*[\s\S]*?\*\//g, "");

describe("theme tokens", () => {
  it("loads the stylesheet", () => {
    expect(typeof stylesheet).toBe("string");
    expect(declarations.length).toBeGreaterThan(1000);
  });

  it("has no self-referencing custom properties", () => {
    const selfReferences = [...declarations.matchAll(/(--[\w-]+)\s*:\s*var\(\s*\1\s*\)/g)];
    expect(selfReferences.map((match) => match[0])).toEqual([]);
  });

  it("defines the default (ink) theme with literal colours", () => {
    for (const token of ["--bg", "--surface", "--raised", "--border", "--accent-light"]) {
      const declaration = new RegExp(`${token}:\\s*([^;]+);`).exec(declarations);
      expect(declaration, `${token} is missing`).not.toBeNull();
      expect(declaration?.[1].trim(), `${token} must not be a var() indirection`).toMatch(
        /^#[0-9a-f]{3,8}$/i,
      );
    }
  });
});
