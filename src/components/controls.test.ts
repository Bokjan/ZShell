import { describe, expect, it } from "vitest";

// The source of every component, to check that none uses a control each platform draws its own way.
const sources = import.meta.glob<string>("../**/*.tsx", { query: "?raw", import: "default", eager: true });

/**
 * Native controls the app draws itself (see "不用原生控件" in docs/ARCHITECTURE.md), with what to
 * use instead, and the files that may still use them. `required`, `pattern` and `minLength` show
 * the web view's own message, in its language rather than the app's, before the form's checks run.
 */
const NATIVE: { pattern: RegExp; instead: string; except?: string[] }[] = [
  { pattern: /<select\b/, instead: "Select" },
  { pattern: /<datalist\b/, instead: "ComboBox" },
  { pattern: /\btype="number"/, instead: "SpinInput" },
  { pattern: /\btype="color"/, instead: "ColorField" },
  { pattern: /\btype="password"/, instead: "PasswordInput" },
  // The compose bar's field is as tall as the lines typed, up to a limit of its own.
  { pattern: /<textarea\b/, instead: "AutoTextarea", except: ["AutoTextarea.tsx", "ComposeBar.tsx"] },
  // An attribute, alone or with a value, followed by another one, the end of the tag or the line.
  { pattern: /\srequired(?=[\s=/>])|\s(?:pattern|minLength)=[{"]/, instead: "the form's own checks" },
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
      return NATIVE.filter(({ pattern, except }) => pattern.test(code) && !except?.some((file) => path.endsWith(`/${file}`))).map(
        ({ pattern, instead }) => `${path}: ${pattern} (use ${instead})`,
      );
    });
    expect(found).toEqual([]);
  });
});
