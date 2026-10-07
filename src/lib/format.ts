import i18n from "../i18n";

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
