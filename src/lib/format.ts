export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return unit === 0 ? `${value} B` : `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Compact timestamp: "10-07 21:30" this year, "2025-10-07" otherwise. */
export function formatTime(seconds: number | null): string {
  if (seconds == null) return "";
  const d = new Date(seconds * 1000);
  const date = `${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  return d.getFullYear() === new Date().getFullYear()
    ? `${date} ${pad(d.getHours())}:${pad(d.getMinutes())}`
    : `${d.getFullYear()}-${date}`;
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
