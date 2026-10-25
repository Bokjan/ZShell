import { describe, expect, it } from "vitest";

import { noticeLines } from "./terminalNotice";

const at = new Date(2026, 2, 5, 7, 8, 9);

describe("noticeLines", () => {
  // The same example as `session/notice.rs`.
  it("starts each line with the time", () => {
    expect(noticeLines("The connection was lost\nPress Enter\n", "error", at)).toBe(
      "\x1b[2m[2026-03-05 07:08:09]\x1b[0m \x1b[31mThe connection was lost\x1b[0m\r\n\x1b[2m[2026-03-05 07:08:09]\x1b[0m \x1b[31mPress Enter\x1b[0m\r\n",
    );
  });

  it("leaves plain text uncolored", () => {
    expect(noticeLines("Sent a.txt\r\n", "plain", at)).toBe("\x1b[2m[2026-03-05 07:08:09]\x1b[0m Sent a.txt\r\n");
  });
});
