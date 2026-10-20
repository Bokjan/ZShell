import { describe, expect, it } from "vitest";

import { matchSuggestions } from "./suggestions";

const ports = [{ value: "COM3", description: "USB Serial Port" }, { value: "COM4" }, { value: "/dev/ttyUSB0" }];

describe("matchSuggestions", () => {
  it("keeps all of them for no text", () => {
    expect(matchSuggestions(ports, "")).toBe(ports);
    expect(matchSuggestions(ports, "  ")).toBe(ports);
  });

  it("matches anywhere in the value or the description, ignoring case", () => {
    expect(matchSuggestions(ports, "com").map((s) => s.value)).toEqual(["COM3", "COM4"]);
    expect(matchSuggestions(ports, "usb").map((s) => s.value)).toEqual(["COM3", "/dev/ttyUSB0"]);
    expect(matchSuggestions(ports, "xterm")).toEqual([]);
  });
});
