import { describe, expect, it } from "vitest";

import { hexToHsv, hsvToHex, parseHex } from "./color";

describe("parseHex", () => {
  it("reads six and three digits, with or without #", () => {
    expect(parseHex("#1E1E2E")).toBe("#1e1e2e");
    expect(parseHex(" 1e1e2e ")).toBe("#1e1e2e");
    expect(parseHex("#abc")).toBe("#aabbcc");
    expect(parseHex("f0a")).toBe("#ff00aa");
  });

  it("rejects anything else", () => {
    expect(parseHex("")).toBeNull();
    expect(parseHex("#12345")).toBeNull();
    expect(parseHex("#1234567")).toBeNull();
    expect(parseHex("red")).toBeNull();
  });
});

describe("hexToHsv", () => {
  it("finds the hue, saturation and brightness", () => {
    expect(hexToHsv("#ff0000")).toEqual({ h: 0, s: 100, v: 100 });
    expect(hexToHsv("#00ff00")).toEqual({ h: 120, s: 100, v: 100 });
    expect(hexToHsv("#0000ff")).toEqual({ h: 240, s: 100, v: 100 });
    expect(hexToHsv("#ff00ff")).toEqual({ h: 300, s: 100, v: 100 });
    expect(hexToHsv("#808080")).toEqual({ h: 0, s: 0, v: (128 / 255) * 100 });
    expect(hexToHsv("#000000")).toEqual({ h: 0, s: 0, v: 0 });
  });
});

describe("hsvToHex", () => {
  it("writes the color", () => {
    expect(hsvToHex({ h: 0, s: 100, v: 100 })).toBe("#ff0000");
    expect(hsvToHex({ h: 60, s: 100, v: 100 })).toBe("#ffff00");
    expect(hsvToHex({ h: 360, s: 100, v: 100 })).toBe("#ff0000");
    expect(hsvToHex({ h: 200, s: 0, v: 100 })).toBe("#ffffff");
    expect(hsvToHex({ h: 200, s: 50, v: 0 })).toBe("#000000");
  });

  it("clamps values out of range", () => {
    expect(hsvToHex({ h: -10, s: 150, v: 120 })).toBe("#ff0000");
  });

  it("round-trips every color it reads", () => {
    for (const hex of ["#1e1e2e", "#7f1d1d", "#fefefe", "#010203", "#3b82f6", "#22c55e"]) {
      expect(hsvToHex(hexToHsv(hex))).toBe(hex);
    }
  });
});
