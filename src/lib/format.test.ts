import i18n from "i18next";
import { beforeAll, describe, expect, it } from "vitest";

import { compareNames, formatAgo, formatList, languageName } from "./format";

describe("formatAgo", () => {
  // In the app's language, which tests set: otherwise it is the system's (LANG), and the
  // test would fail on a Chinese system.
  beforeAll(() => i18n.init({ lng: "en", resources: {} }));

  it("says how long ago in the largest whole unit", () => {
    expect(formatAgo(0)).toBe("0 seconds ago");
    expect(formatAgo(1999)).toBe("1 second ago");
    expect(formatAgo(59_999)).toBe("59 seconds ago");
    expect(formatAgo(60_000)).toBe("1 minute ago");
    expect(formatAgo(2 * 3600_000 + 5)).toBe("2 hours ago");
    expect(formatAgo(3 * 86400_000)).toBe("3 days ago");
  });
});

describe("languageName", () => {
  it("names a language in itself, capitalized", () => {
    expect(languageName("en")).toBe("English");
    expect(languageName("zh-Hans")).toBe("简体中文");
    expect(languageName("ja")).toBe("日本語");
    expect(languageName("de")).toBe("Deutsch");
    expect(languageName("fr")).toBe("Français");
  });
});

describe("formatList", () => {
  it("writes the list as the interface language does", async () => {
    await i18n.init({ lng: "en", resources: {} });
    expect(formatList(["a", "b", "c"])).toBe("a, b, and c");
    expect(formatList(["a", "b", "c"], "narrow")).toBe("a, b, c");
    await i18n.changeLanguage("zh-Hans");
    expect(formatList(["a", "b", "c"])).toBe("a、b和c");
    expect(formatList(["a", "b", "c"], "narrow")).toBe("a、b、c");
    await i18n.changeLanguage("en");
  });
});

describe("compareNames", () => {
  it("puts numbers in order and ignores case, in the interface language", async () => {
    await i18n.init({ lng: "en", resources: {} });
    expect(["file10", "File2", "file1"].sort(compareNames)).toEqual(["file1", "File2", "file10"]);
    // Swedish sorts "ä" after "z"; English with "a".
    expect(["ä", "z"].sort(compareNames)).toEqual(["ä", "z"]);
    await i18n.changeLanguage("sv");
    expect(["ä", "z"].sort(compareNames)).toEqual(["z", "ä"]);
    await i18n.changeLanguage("en");
  });
});
