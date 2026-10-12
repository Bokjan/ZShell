import type { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";

import { extraModes } from "./terminalModes";

const term = (core: object, rows = 24) => ({ rows, _core: core }) as unknown as Terminal;

describe("extraModes", () => {
  it("adds nothing for a terminal as it starts", () => {
    const core = {
      coreMouseService: { activeEncoding: "DEFAULT" },
      coreService: { isCursorHidden: false, decPrivateModes: {} },
      buffer: { scrollTop: 0, scrollBottom: 23 },
    };
    expect(extraModes(term(core))).toBe("");
  });

  it("keeps the mouse encoding, a hidden cursor, its shape and the scrolling region", () => {
    const core = {
      coreMouseService: { activeEncoding: "SGR" },
      coreService: { isCursorHidden: true, decPrivateModes: { cursorStyle: "bar", cursorBlink: false } },
      buffer: { scrollTop: 1, scrollBottom: 22 },
    };
    expect(extraModes(term(core))).toBe("\x1b[?1006h\x1b[?25l\x1b[6 q\x1b7\x1b[2;23r\x1b8");
  });

  it("adds nothing if xterm.js's internals have changed", () => {
    expect(extraModes(term({}))).toBe("");
    expect(extraModes({ rows: 24 } as unknown as Terminal)).toBe("");
  });
});
