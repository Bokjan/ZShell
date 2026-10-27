import { describe, expect, it } from "vitest";

import { isKey, usCode } from "./keys";

describe("usCode", () => {
  it("finds the US key of letters and punctuation", () => {
    expect(usCode("e")).toBe("KeyE");
    expect(usCode(",")).toBe("Comma");
    expect(usCode("[")).toBe("BracketLeft");
    expect(usCode("1")).toBeUndefined();
  });
});

describe("isKey", () => {
  it("goes by the character typed", () => {
    expect(isKey({ key: "e", code: "KeyE" }, "e")).toBe(true);
    // With Shift.
    expect(isKey({ key: "E", code: "KeyE" }, "e")).toBe(true);
    // AZERTY: A on the US Q key, and the comma on the US M key isn't M.
    expect(isKey({ key: "a", code: "KeyQ" }, "a")).toBe(true);
    expect(isKey({ key: "q", code: "KeyA" }, "a")).toBe(false);
    expect(isKey({ key: ",", code: "KeyM" }, "m")).toBe(false);
    expect(isKey({ key: ",", code: "KeyM" }, ",")).toBe(true);
    // QWERTZ and Dvorak.
    expect(isKey({ key: "z", code: "KeyY" }, "z")).toBe(true);
    expect(isKey({ key: "f", code: "KeyY" }, "f")).toBe(true);
    // A Japanese keyboard's brackets.
    expect(isKey({ key: "[", code: "BracketRight" }, "[")).toBe(true);
    expect(isKey({ key: "]", code: "Backslash" }, "]")).toBe(true);
  });

  it("goes by the US key where the layout types no ASCII there", () => {
    // Russian.
    expect(isKey({ key: "а", code: "KeyF" }, "f")).toBe(true);
    expect(isKey({ key: "х", code: "BracketLeft" }, "[")).toBe(true);
    expect(isKey({ key: "а", code: "KeyF" }, "a")).toBe(false);
    // Option on macOS, a dead key, an input method.
    expect(isKey({ key: "ƒ", code: "KeyF" }, "f")).toBe(true);
    expect(isKey({ key: "Dead", code: "BracketLeft" }, "[")).toBe(true);
    expect(isKey({ key: "Process", code: "KeyA" }, "a")).toBe(true);
  });
});
