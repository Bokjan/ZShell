import { describe, expect, it } from "vitest";

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

const isLiteral = (text: string) => ["\"", "'", "`"].some((quote) => sources.includes(`${quote}${text}${quote}`));

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
