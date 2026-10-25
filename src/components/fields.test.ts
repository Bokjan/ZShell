import { describe, expect, it } from "vitest";

// The source of every component, to check that the titles of fields are all written with `Field`.
const sources = import.meta.glob<string>("../**/*.tsx", { query: "?raw", import: "default", eager: true });

/**
 * Titles written by hand, which would look different from the others: a `<label>` that starts
 * with its text (a checkbox's starts with its `<input>`), a `<legend>` that is shown, or a span
 * in a `.field`. `Field` writes them, and `PasswordInput` its own label (see there). The color
 * picker's "Hex" sits beside its field, as part of the picker, rather than above it as a title.
 */
const BY_HAND = [/<label\b(?![^>]*color-hex)[^>]*>\s*[^<\s]/, /<legend\b(?![^>]*visually-hidden)/, /className="field"[^>]*>\s*<span\b/];
const ALLOWED = ["Field.tsx", "PasswordInput.tsx"];

// Comments may show how titles were written before.
const withoutComments = (source: string) =>
  source
    .replace(/\{\s*\/\*[\s\S]*?\*\/\s*\}/g, "")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

describe("field titles", () => {
  it("are found in the components", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(20);
  });

  it("are written with Field", () => {
    const found = Object.entries(sources)
      .filter(([path]) => !ALLOWED.some((file) => path.endsWith(`/${file}`)))
      .flatMap(([path, source]) =>
        BY_HAND.filter((pattern) => pattern.test(withoutComments(source))).map((pattern) => `${path}: ${pattern}`),
      );
    expect(found).toEqual([]);
  });
});
