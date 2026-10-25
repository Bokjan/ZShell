/**
 * Our own messages in the terminal (reconnecting, how to go on), each line starting with the
 * time it is printed, as the backend writes its own (`session/notice.rs`); the tests of both
 * check the same examples.
 */

import type { TimePart } from "../ipc/api";

/** How a message's text is shown; the time before it is always dim. */
export type Tone = "plain" | "dim" | "warning" | "error";

const SGR: Record<Tone, string> = { plain: "", dim: "\x1b[2m", warning: "\x1b[33m", error: "\x1b[31m" };

const pad = (n: number, digits = 2) => String(n).padStart(digits, "0");

/** `at` in a time format, as the backend parsed it (`model/time_format.rs`). */
export function formatTime(parts: TimePart[], at: Date): string {
  return parts
    .map((part) => {
      switch (part.type) {
        case "text":
          return part.text;
        case "year":
          return pad(at.getFullYear(), 4);
        case "shortYear":
          return pad(at.getFullYear() % 100);
        case "month":
          return pad(at.getMonth() + 1);
        case "day":
          return pad(at.getDate());
        case "hour":
          return pad(at.getHours());
        case "minute":
          return pad(at.getMinutes());
        case "second":
          return pad(at.getSeconds());
        case "millisecond":
          return pad(at.getMilliseconds(), 3);
      }
    })
    .join("");
}

/** `text` as written to the terminal, each line starting with `time`, and ending a line. */
export function noticeLines(text: string, tone: Tone, time: string): string {
  const stamp = `\x1b[2m${time}\x1b[0m `;
  const lines = text.replace(/[\r\n]+$/, "").split(/\r?\n/);
  return lines.map((line) => stamp + (tone === "plain" ? line : `${SGR[tone]}${line}\x1b[0m`) + "\r\n").join("");
}
