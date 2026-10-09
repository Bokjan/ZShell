import i18n from "../i18n";
import type { ForwardRule } from "./api";

/**
 * The number typed in a field that takes whole numbers, or NaN: digits only, as `Number`
 * would also take "0x16", "1e3" and "1.0".
 */
export function wholeNumber(text: string): number {
  const trimmed = text.trim();
  return /^\d+$/.test(trimmed) ? Number(trimmed) : NaN;
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

/** `ls -l` style mode string, e.g. "drwxr-xr-x". */
export function formatMode(mode: number | null, isDir: boolean, isSymlink: boolean): string {
  if (mode == null) return "";
  const type = isSymlink ? "l" : isDir ? "d" : "-";
  const bits = "rwxrwxrwx";
  return type + [...bits].map((c, i) => (mode & (1 << (8 - i)) ? c : "-")).join("");
}

export function basename(path: string): string {
  return path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() ?? path;
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
