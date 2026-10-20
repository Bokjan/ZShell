import { describe, expect, it } from "vitest";

// The source of every component, to check that none uses a control each platform draws its own way.
const sources = import.meta.glob<string>("../**/*.tsx", { query: "?raw", import: "default", eager: true });

/**
 * Native controls the app draws itself (see "不用原生控件" in docs/ARCHITECTURE.md), with what to
 * use instead. `required`, `pattern` and `minLength` show the web view's own message, in its language rather
 * than the app's, before the form's checks run.
 */
const NATIVE = [
  { pattern: /<select\b/, instead: "Select" },
  { pattern: /<datalist\b/, instead: "ComboBox" },
  { pattern: /\btype="number"/, instead: "SpinInput" },
  { pattern: /\btype="color"/, instead: "ColorField" },
  // An attribute alone on its line, or followed by another one or the end of the tag.
  { pattern: /^\s*required\s*$|\srequired(?=\s+\w+=|\s*\/?>)|\s(?:pattern|minLength)=[{"]/m, instead: "the form's own checks" },
];

// Comments may name the controls they replace.
const withoutComments = (source: string) => source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

describe("native controls", () => {
  it("are found in the components", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(20);
  });

  it("are never used", () => {
    const found = Object.entries(sources).flatMap(([path, source]) => {
      const code = withoutComments(source);
      return NATIVE.filter(({ pattern }) => pattern.test(code)).map(({ pattern, instead }) => `${path}: ${pattern} (use ${instead})`);
    });
    expect(found).toEqual([]);
  });
});
