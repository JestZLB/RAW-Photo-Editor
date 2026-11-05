import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    passWithNoTests: true,
    // `styles.test.ts` reads the real stylesheet via `?raw`; Vitest replaces
    // CSS imports with empty strings unless CSS processing is enabled.
    css: true,
  },
});
