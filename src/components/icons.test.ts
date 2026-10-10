import { describe, expect, it } from "vitest";

// The source of every component, to check that icons come only from icons.tsx.
const sources = import.meta.glob<string>(["../**/*.tsx", "!./icons.tsx"], { query: "?raw", import: "default", eager: true });

/**
 * Characters that are icons when they stand alone in the UI: arrows, geometric shapes, the
 * multiplication sign, check marks, dingbats and emoji. The keys of shortcut labels (⌘ ⌥ ⌃ ⇧ ⌫)
 * are text, and the Windows window buttons use Segoe glyphs (private use code points).
 */
const ICON_CHARACTER = /(?![⌘⌥⌃⇧⌫])[\u00D7\u2190-\u21FF\u2300-\u23FF\u25A0-\u27BF\u2B00-\u2BFF]|\p{Extended_Pictographic}/u;

// Comments may mention the characters (keys, the glyphs an icon replaces).
const withoutComments = (source: string) => source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

describe("icons", () => {
  it("are found in the components", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(20);
  });

  it("are drawn only in icons.tsx", () => {
    const drawn = Object.entries(sources)
      .filter(([, source]) => /<svg\b/.test(withoutComments(source)))
      .map(([path]) => path);
    expect(drawn).toEqual([]);
  });

  it("are never characters", () => {
    const found = Object.entries(sources).flatMap(([path, source]) =>
      withoutComments(source)
        .split("\n")
        .filter((line) => ICON_CHARACTER.test(line))
        .map((line) => `${path}: ${line.trim()}`),
    );
    expect(found).toEqual([]);
  });
});
