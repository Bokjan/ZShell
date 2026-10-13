import { describe, expect, it } from "vitest";

import { LANGUAGES } from "../lib/bindings";
import en from "./en.json";

// Every source but the tests, including the generated bindings, whose string unions name the
// values that dynamic keys are built from.
const sources = Object.values(
  import.meta.glob<string>(["../**/*.{ts,tsx}", "!../**/*.test.ts"], { query: "?raw", import: "default", eager: true }),
).join("\n");

/** The catalog's keys as dotted paths. */
function keys(value: object, prefix = ""): string[] {
  return Object.entries(value).flatMap(([key, child]) =>
    typeof child === "object" && child !== null ? keys(child, `${prefix}${key}.`) : [`${prefix}${key}`],
  );
}

// i18next picks `key_one`, `key_other`… for `t("key", { count })`.
const PLURAL = /_(zero|one|two|few|many|other)$/;

const isLiteral = (text: string) => ['"', "'", "`"].some((quote) => sources.includes(`${quote}${text}${quote}`));

// `t(`proxy.kinds.${kind}`)`: the start of a key whose end is a value from the sources.
const dynamicPrefixes = [...sources.matchAll(/`([\w.]+\.)\$\{/g)].map((match) => match[1]);

/**
 * A key is used when the sources spell it out, or when a dynamic key's prefix leads to it and
 * the rest of it is a string in the sources (`proxy.kinds.` and the proxy kind "socks5").
 */
const isUsed = (key: string) => {
  const base = key.replace(PLURAL, "");
  return isLiteral(base) || dynamicPrefixes.some((prefix) => base.startsWith(prefix) && isLiteral(base.slice(prefix.length)));
};

describe("en.json", () => {
  it("has no unused keys", () => {
    expect(keys(en).filter((key) => !isUsed(key))).toEqual([]);
  });
});

/** The catalogs other than English, by language. */
const translations = Object.fromEntries(
  Object.entries(import.meta.glob<object>("./*.json", { import: "default", eager: true }))
    .map(([path, catalog]) => [path.slice(2, -".json".length), catalog] as const)
    .filter(([language]) => language !== "en"),
);

/** The catalog's strings by dotted key. */
const strings = (catalog: object) =>
  new Map(
    keys(catalog).map((key) => [key, key.split(".").reduce<unknown>((value, part) => (value as Record<string, unknown>)[part], catalog)]),
  );

const placeholders = (text: string) => [...new Set([...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]))].sort();

describe("catalogs", () => {
  it("are the languages the backend has", () => {
    expect(["en", ...Object.keys(translations)].sort()).toEqual([...LANGUAGES].sort());
  });

  const source = strings(en);
  // The English strings by key, with plural keys under their base and their forms' placeholders merged.
  const plurals = new Map<string, string[]>();
  for (const [key, text] of source) {
    if (!PLURAL.test(key)) continue;
    const base = key.replace(PLURAL, "");
    plurals.set(base, [...new Set([...(plurals.get(base) ?? []), ...placeholders(text as string)])].sort());
  }

  for (const [language, catalog] of Object.entries(translations)) {
    describe(language, () => {
      const translated = strings(catalog);
      const categories = new Intl.PluralRules(language).resolvedOptions().pluralCategories;

      it("has every key of en.json and no other", () => {
        const expected = [
          ...[...source.keys()].filter((key) => !PLURAL.test(key)),
          // i18next takes `_zero` for a count of 0 in any language, where English has one.
          ...[...plurals.keys()].flatMap((base) =>
            [...categories, ...(source.has(`${base}_zero`) && !categories.includes("zero") ? ["zero"] : [])].map(
              (category) => `${base}_${category}`,
            ),
          ),
        ].sort();
        expect([...translated.keys()].sort()).toEqual(expected);
      });

      it("has every string translated, with the placeholders of en.json", () => {
        const wrong: string[] = [];
        for (const [key, text] of translated) {
          if (typeof text !== "string" || text.trim() === "") {
            wrong.push(`${key}: empty`);
            continue;
          }
          const base = key.replace(PLURAL, "");
          const expected = PLURAL.test(key) ? plurals.get(base) : placeholders(source.get(key) as string);
          // A plural form may leave out the count ("Un fichier"), not the other placeholders.
          const actual = placeholders(text);
          const ok = PLURAL.test(key)
            ? actual.every((name) => expected?.includes(name)) &&
              (expected ?? []).every((name) => name === "count" || actual.includes(name))
            : actual.join() === expected?.join();
          if (!ok) wrong.push(`${key}: {${actual.join("}, {")}} instead of {${(expected ?? []).join("}, {")}}`);
        }
        expect(wrong).toEqual([]);
      });
    });
  }
});
