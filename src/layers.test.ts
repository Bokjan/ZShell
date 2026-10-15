import { describe, expect, it } from "vitest";

// The sources of `lib/`: logic without React, reaching the backend only through `ipc/`.
const lib = import.meta.glob<string>(["./lib/**/*.{ts,tsx}", "!./lib/**/*.test.ts"], { query: "?raw", import: "default", eager: true });
// Every source outside `ipc/`, the only place that calls commands and listens to the backend.
const outside = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./ipc/**", "!./**/*.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});
const ipc = import.meta.glob<string>(["./ipc/**/*.ts", "!./ipc/**/*.test.ts"], { query: "?raw", import: "default", eager: true });

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

describe("ipc", () => {
  it("is found", () => {
    expect(Object.keys(outside).length).toBeGreaterThan(50);
    expect(Object.keys(ipc).length).toBeGreaterThan(3);
  });

  it("is the only place that calls commands or listens to events", () => {
    // Tauri's own window and plugin APIs come with their types; these carry ours.
    const backend = ["@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/api/webviewWindow"];
    const found = Object.entries(outside).flatMap(([path, source]) =>
      imports(source)
        .filter((module) => backend.includes(module))
        .map((module) => `${path}: ${module}`),
    );
    expect(found).toEqual([]);
  });

  it("calls commands only through `call`, which checks their names", () => {
    const found = Object.entries(ipc)
      .filter(([path, source]) => path !== "./ipc/call.ts" && /\binvoke\b/.test(source))
      .map(([path]) => path);
    expect(found).toEqual([]);
  });
});
