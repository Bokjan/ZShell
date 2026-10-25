import { describe, expect, it } from "vitest";

import { DEFAULT_TIME_FORMAT_PARTS, type TimePart } from "../ipc/api";
import { formatTime, noticeLines } from "./terminalNotice";

const at = new Date(2026, 2, 5, 7, 8, 9, 45);

// The same examples as `logging/time_format.rs` and `session/notice.rs`.
describe("formatTime", () => {
  it("writes each part", () => {
    expect(formatTime(DEFAULT_TIME_FORMAT_PARTS, at)).toBe("[2026-03-05 07:08:09]");
    // "%y/%m/%d %H:%M:%S.%3f", as the backend parses it.
    const parts: TimePart[] = [
      { type: "shortYear" },
      { type: "text", text: "/" },
      { type: "month" },
      { type: "text", text: "/" },
      { type: "day" },
      { type: "text", text: " " },
      { type: "hour" },
      { type: "text", text: ":" },
      { type: "minute" },
      { type: "text", text: ":" },
      { type: "second" },
      { type: "text", text: "." },
      { type: "millisecond" },
    ];
    expect(formatTime(parts, at)).toBe("26/03/05 07:08:09.045");
  });
});

describe("noticeLines", () => {
  const time = "[2026-03-05 07:08:09]";

  it("starts each line with the time", () => {
    expect(noticeLines("The connection was lost\nPress Enter\n", "error", time)).toBe(
      "\x1b[2m[2026-03-05 07:08:09]\x1b[0m \x1b[31mThe connection was lost\x1b[0m\r\n\x1b[2m[2026-03-05 07:08:09]\x1b[0m \x1b[31mPress Enter\x1b[0m\r\n",
    );
  });

  it("leaves plain text uncolored", () => {
    expect(noticeLines("Sent a.txt\r\n", "plain", time)).toBe("\x1b[2m[2026-03-05 07:08:09]\x1b[0m Sent a.txt\r\n");
  });
});
