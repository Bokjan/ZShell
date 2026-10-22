import i18n from "../i18n/index";
import type { ForwardRule } from "../ipc/api";

/**
 * The number typed in a field that takes whole numbers, or NaN: digits only, as `Number`
 * would also take "0x16", "1e3" and "1.0".
 */
export function wholeNumber(text: string): number {
  const trimmed = text.trim();
  return /^\d+$/.test(trimmed) ? Number(trimmed) : NaN;
}

/** A TCP port as typed (digits only: not `0x16` or `1e3`), 1–65535, or 0 too if allowed; else null. */
export function parsePort(text: string, { allowZero = false } = {}): number | null {
  const port = wholeNumber(text);
  return port >= (allowZero ? 0 : 1) && port <= 65535 ? port : null;
}

/** Binary-prefixed size with locale-aware digits, e.g. "1.5 MB" ("1,5 MB" in German). */
export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  const number = new Intl.NumberFormat(i18n.language, { maximumFractionDigits: digits }).format(value);
  return `${number} ${units[unit]}`;
}

/** Compact, locale-formatted timestamp: date and time this year, date only for older files. */
export function formatTime(seconds: number | null): string {
  if (seconds == null) return "";
  const date = new Date(seconds * 1000);
  const options: Intl.DateTimeFormatOptions =
    date.getFullYear() === new Date().getFullYear()
      ? { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }
      : { year: "numeric", month: "2-digit", day: "2-digit" };
  return new Intl.DateTimeFormat(i18n.language, options).format(date);
}

/**
 * Items as a list in a sentence, as the interface language writes one: "a, b, and c" in English,
 * "a、b和c" in Chinese. `narrow` leaves out the conjunction where the language can ("a、b、c"):
 * for a list that isn't part of a sentence.
 */
export function formatList(items: string[], style: "long" | "narrow" = "long"): string {
  return new Intl.ListFormat(i18n.language, { type: "conjunction", style }).format(items);
}

let collator: { language: string; compare: (a: string, b: string) => number } | null = null;

/** Compares names as people sort them in the interface language: "file2" before "file10", ignoring case. */
export function compareNames(a: string, b: string): number {
  if (collator === null || collator.language !== i18n.language) {
    collator = { language: i18n.language, compare: new Intl.Collator(i18n.language, { numeric: true, sensitivity: "base" }).compare };
  }
  return collator.compare(a, b);
}

/** How long ago something happened, given in milliseconds: "5 seconds ago", "2 minutes ago". */
export function formatAgo(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const format = new Intl.RelativeTimeFormat(i18n.language, { numeric: "always" });
  if (seconds < 60) return format.format(-seconds, "second");
  if (seconds < 3600) return format.format(-Math.floor(seconds / 60), "minute");
  if (seconds < 86400) return format.format(-Math.floor(seconds / 3600), "hour");
  return format.format(-Math.floor(seconds / 86400), "day");
}

/**
 * A language's name in that language ("Deutsch", "日本語"), capitalized as a list item: French
 * writes "français" in a sentence.
 */
export function languageName(language: string): string {
  const name = new Intl.DisplayNames([language], { type: "language" }).of(language) ?? language;
  return name.charAt(0).toLocaleUpperCase(language) + name.slice(1);
}

/** `ls -l` style mode string, e.g. "drwxr-xr-x". */
export function formatMode(mode: number | null, isDir: boolean, isSymlink: boolean): string {
  if (mode == null) return "";
  const type = isSymlink ? "l" : isDir ? "d" : "-";
  const bits = "rwxrwxrwx";
  return type + [...bits].map((c, i) => (mode & (1 << (8 - i)) ? c : "-")).join("");
}

export function basename(path: string): string {
  return (
    path
      .replace(/[/\\]+$/, "")
      .split(/[/\\]/)
      .pop() ?? path
  );
}

/** `host:port`, bracketing IPv6 literals; port 0 ("pick a free port") is shown as `*`. */
export function hostPort(host: string, port: number): string {
  const h = host.includes(":") ? `[${host}]` : host;
  return `${h}:${port === 0 ? "*" : port}`;
}

/** What a forwarding rule maps: `bind → target`, or the bind address of a dynamic rule. */
export function forwardMapping(rule: ForwardRule): string {
  const bind = hostPort(rule.bindHost, rule.bindPort);
  return rule.kind === "dynamic" ? bind : `${bind} → ${hostPort(rule.targetHost, rule.targetPort)}`;
}
