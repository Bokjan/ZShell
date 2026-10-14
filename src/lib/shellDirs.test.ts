import { describe, expect, it } from "vitest";

import { parseOsc7 } from "./shellDirs";

describe("parseOsc7", () => {
  it("takes the path of a file URL, decoded", () => {
    expect(parseOsc7("file://web01/srv/www")).toBe("/srv/www");
    expect(parseOsc7("file:///home/alice/My%20Files")).toBe("/home/alice/My Files");
    expect(parseOsc7("file://web01/home/alice/%E6%96%87%E6%A1%A3")).toBe("/home/alice/文档");
  });

  it("ignores anything else", () => {
    expect(parseOsc7("file://web01")).toBeNull();
    expect(parseOsc7("http://example.com/x")).toBeNull();
    expect(parseOsc7("file:///bad%ZZ%E6")).toBeNull();
  });
});
