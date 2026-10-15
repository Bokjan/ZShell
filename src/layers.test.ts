import { describe, expect, it } from "vitest";

// The sources of `lib/`: logic without React, reaching the backend only through `ipc/`.
const lib = import.meta.glob<string>(["./lib/**/*.{ts,tsx}", "!./lib/**/*.test.ts"], { query: "?raw", import: "default", eager: true });

// Type imports are erased: a module may name React's types.
const imports = (source: string) => [...source.matchAll(/^import (?!type )[^;]*?from "([^"]+)";/gms)].map((match) => match[1]);

describe("lib", () => {
  it("is found", () => {
    expect(Object.keys(lib).length).toBeGreaterThan(20);
  });

  it("uses neither React nor Tauri", () => {
    const found = Object.entries(lib).flatMap(([path, source]) =>
      imports(source)
        .filter((module) => module === "react" || module.startsWith("react-") || module.startsWith("@tauri-apps/"))
        .map((module) => `${path}: ${module}`),
    );
    expect(found).toEqual([]);
  });
});
