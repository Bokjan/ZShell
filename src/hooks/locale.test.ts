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

/**
 * The helpers `source` imports or re-exports from their modules, in any form: named (also
 * renamed, or beside a default import), or the whole module as a namespace ("* as format").
 */
function helpersImported(source: string): string[] {
  return [...source.matchAll(/^(?:import|export)\s+(?!type\s)([^;]*?)\s+from\s+"([^"]+)";/gms)].flatMap(([, clause, module]) => {
    const helpers = Object.entries(HELPERS).find(([suffix]) => module.endsWith(suffix))?.[1] ?? [];
    if (helpers.length === 0) return [];
    if (/\*\s+as\s+\w+/.test(clause)) return [`* from ${module}`];
    const names = /\{([^}]*)\}/.exec(clause)?.[1] ?? "";
    return names
      .split(",")
      .map((name) => name.trim().split(/\s+as\s+/)[0])
      .filter((name) => helpers.includes(name));
  });
}

describe("helpersImported", () => {
  it("finds named imports, also renamed or beside a default import", () => {
    expect(helpersImported(`import { basename, formatSize } from "../lib/format";`)).toEqual(["formatSize"]);
    expect(helpersImported(`import { formatAgo as ago } from "../lib/format";`)).toEqual(["formatAgo"]);
    expect(helpersImported(`import x, {\n  shortcutLabel,\n} from "../lib/keymap";`)).toEqual(["shortcutLabel"]);
  });

  it("finds namespace imports and re-exports", () => {
    expect(helpersImported(`import * as format from "../lib/format";`)).toEqual(["* from ../lib/format"]);
    expect(helpersImported(`export { currentCjk } from "../i18n/index";`)).toEqual(["currentCjk"]);
  });

  it("leaves out other names, modules and type-only imports", () => {
    expect(helpersImported(`import { basename } from "../lib/format";`)).toEqual([]);
    expect(helpersImported(`import { formatSize } from "../lib/other";`)).toEqual([]);
    expect(helpersImported(`import type { Action } from "../lib/keymap";`)).toEqual([]);
  });
});

describe("useLocale", () => {
  it("finds the sources", () => {
    expect(Object.keys(rendering).length).toBeGreaterThan(50);
  });

  it("is where components and hooks get the helpers that depend on the language", () => {
    const found = Object.entries(rendering).flatMap(([path, source]) => helpersImported(source).map((name) => `${path}: ${name}`));
    expect(found).toEqual([]);
  });
});
