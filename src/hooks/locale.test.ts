import { describe, expect, it } from "vitest";

// Components and the hooks they use: what renders.
const rendering = import.meta.glob<string>(
  ["../App.tsx", "../app/**/*.{ts,tsx}", "../components/**/*.{ts,tsx}", "./**/*.{ts,tsx}", "!./locale.ts", "!../**/*.test.ts"],
  { query: "?raw", import: "default", eager: true },
);

// Their text depends on the interface language through state outside React (see `useLocale`).
const HELPERS: Record<string, string[]> = {
  "lib/keymap": ["shortcutLabel", "shortcutRows"],
  "lib/format": ["formatSize", "formatTime", "formatAgo"],
  "i18n/index": ["currentCjk"],
};

describe("useLocale", () => {
  it("finds the sources", () => {
    expect(Object.keys(rendering).length).toBeGreaterThan(50);
  });

  it("is where components and hooks get the helpers that depend on the language", () => {
    const found = Object.entries(rendering).flatMap(([path, source]) =>
      [...source.matchAll(/^import \{([^}]*)\} from "([^"]+)";/gm)].flatMap(([, names, module]) => {
        const helpers = Object.entries(HELPERS).find(([suffix]) => module.endsWith(suffix))?.[1] ?? [];
        return names
          .split(",")
          .map((name) => name.trim())
          .filter((name) => helpers.includes(name))
          .map((name) => `${path}: ${name}`);
      }),
    );
    expect(found).toEqual([]);
  });
});
