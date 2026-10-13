import i18n from "i18next";
import { beforeAll, describe, expect, it } from "vitest";

import { formatAgo, languageName } from "./format";

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
