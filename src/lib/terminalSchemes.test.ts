import { describe, expect, it } from "vitest";

import { fontStack, resolveScheme, sessionScheme, setSystemFont, systemFontFamilies, tabMark } from "./terminalSchemes";

describe("resolveScheme", () => {
  it("finds a scheme by id, and follows the appearance for auto or an unknown id", () => {
    expect(resolveScheme("nord", "light").id).toBe("nord");
    expect(resolveScheme("auto", "dark").id).toBe("default-dark");
    expect(resolveScheme("removed", "light").id).toBe("default-light");
  });
});

describe("sessionScheme", () => {
  it("prefers the session's own scheme to the setting's", () => {
    expect(sessionScheme("nord", "dark", undefined).id).toBe("nord");
    expect(sessionScheme("nord", "dark", { colorScheme: "dracula" }).id).toBe("dracula");
    expect(sessionScheme("dracula", "light", { colorScheme: "auto" }).id).toBe("default-light");
  });

  it("puts the session's background in, dark or not by its luminance", () => {
    const light = resolveScheme("default-light", "light");
    const red = sessionScheme("default-light", "light", { background: "#300000" });
    expect(red.dark).toBe(true);
    expect(red.theme).toEqual({ ...light.theme, background: "#300000" });
    expect(sessionScheme("default-dark", "dark", { background: "#808080" }).dark).toBe(false);
    expect(sessionScheme("default-light", "light", { background: "#606060" }).dark).toBe(true);
  });
});

describe("tabMark", () => {
  it("keeps the hue at a middle lightness", () => {
    expect(tabMark("#ff0000")).toBe("hsl(0 100% 55%)");
    expect(tabMark("#00ff00")).toBe("hsl(120 100% 55%)");
    expect(tabMark("#0000ff")).toBe("hsl(240 100% 55%)");
    expect(tabMark("#ff00ff")).toBe("hsl(300 100% 55%)");
    expect(tabMark("#330000")).toBe("hsl(0 100% 55%)");
  });

  it("gives a faint color at least half saturation, and leaves grays gray", () => {
    expect(tabMark("#806060")).toBe("hsl(0 50% 55%)");
    expect(tabMark("#808080")).toBe("hsl(0 0% 55%)");
  });
});

describe("fontStack", () => {
  it("puts the preferred font first, then the desktop's monospace font", () => {
    expect(fontStack("", "zh-Hans")).toMatch(/^Menlo, /);
    expect(fontStack("Fira Code", "zh-Hans")).toMatch(/^Fira Code, Menlo, /);
    setSystemFont('Ubuntu Sans Mono, Noto "Mono"');
    expect(systemFontFamilies()).toBe('"Ubuntu Sans Mono", "Noto Mono"');
    expect(fontStack("", "zh-Hans")).toMatch(/^"Ubuntu Sans Mono", "Noto Mono", Menlo, /);
    expect(fontStack("Fira Code", "zh-Hans")).toMatch(/^Fira Code, "Ubuntu Sans Mono", "Noto Mono", Menlo, /);
    expect(fontStack("", "zh-Hans")).toMatch(/, monospace$/);
  });

  it("puts the CJK fonts of the language last", () => {
    expect(fontStack("", "zh-Hans")).toMatch(/"Noto Sans Mono CJK SC", monospace$/);
    expect(fontStack("", "ja")).toMatch(/"Hiragino Sans", "Yu Gothic", "Noto Sans Mono CJK JP", monospace$/);
  });
});
