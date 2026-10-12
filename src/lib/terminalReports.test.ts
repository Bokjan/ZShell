import { describe, expect, it } from "vitest";

import { isTerminalReport } from "./terminalReports";

describe("isTerminalReport", () => {
  it("recognizes what the terminal sends for programs", () => {
    const reports = [
      "\x1b[<0;12;5M", // SGR mouse press
      "\x1b[<0;12;5m", // and release
      "\x1b[M !!", // X10 mouse
      "\x1b[I", // focus in
      "\x1b[O", // focus out
      "\x1b[?1;2c", // primary device attributes
      "\x1b[>0;276;0c", // secondary device attributes
      "\x1b[12;40R", // cursor position
      "\x1b[?12;40R", // DEC cursor position
      "\x1b[?2004;2$y", // mode
      "\x1b[0n", // status
      "\x1b[8;24;80t", // window size
      "\x1b]11;rgb:0000/0000/0000\x07", // OSC color, ended by BEL
      "\x1b]11;rgb:0000/0000/0000\x1b\\", // and by ST
      "\x1bP1$r0m\x1b\\", // DCS
    ];
    for (const report of reports) expect(isTerminalReport(report), JSON.stringify(report)).toBe(true);
  });

  it("leaves what the user types alone", () => {
    const typed = ["a", "ls\r", "\x1b", "\x1b[A", "\x1bOA", "\x1b[3~", "\x1b[15;5~", "\x1b[<0;12;5M\x1b[A", "\x1b]11;unterminated"];
    for (const data of typed) expect(isTerminalReport(data), JSON.stringify(data)).toBe(false);
  });

  it("counts Shift+F3 as a cursor position report, which it can't be told apart from", () => {
    expect(isTerminalReport("\x1b[1;2R")).toBe(true);
  });
});
