/**
 * Our own messages in the terminal (reconnecting, how to go on), each line starting with the
 * time it is printed, as the backend writes its own (`session/notice.rs`); the tests of both
 * check the same example.
 */

/** How a message's text is shown; the time before it is always dim. */
export type Tone = "plain" | "dim" | "warning" | "error";

const SGR: Record<Tone, string> = { plain: "", dim: "\x1b[2m", warning: "\x1b[33m", error: "\x1b[31m" };

const pad = (n: number) => String(n).padStart(2, "0");

/** `text` as written to the terminal, each line starting with `now`, and ending a line. */
export function noticeLines(text: string, tone: Tone, now: Date): string {
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
  const stamp = `\x1b[2m[${date} ${time}]\x1b[0m `;
  const lines = text.replace(/[\r\n]+$/, "").split(/\r?\n/);
  return lines.map((line) => stamp + (tone === "plain" ? line : `${SGR[tone]}${line}\x1b[0m`) + "\r\n").join("");
}
