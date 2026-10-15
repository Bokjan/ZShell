import { isWindows } from "./platform";

const separator = isWindows ? "\\" : "/";

/** Whether a path starts with `~`, which the backend replaces with the home folder. */
export const startsWithHome = (path: string) => path === "~" || path.startsWith("~/") || path.startsWith("~\\");

/** `~/…` with the home folder in place of `~`, written with this platform's separators. */
export function expandHome(path: string, home: string) {
  if (!startsWithHome(path)) return path;
  const rest = path.slice(2).replace(/[\\/]+/g, separator);
  return rest ? `${home.replace(/[\\/]+$/, "")}${separator}${rest}` : home;
}

/**
 * A path in the home folder as `~/…` (with forward slashes, which Windows accepts too), so that
 * an exported session works on another computer; other paths as they are.
 */
export function contractHome(path: string, home: string) {
  const base = home.replace(/[\\/]+$/, "");
  const inside = isWindows ? path.toLowerCase().startsWith(`${base.toLowerCase()}\\`) : path.startsWith(`${base}/`);
  return inside ? `~/${path.slice(base.length + 1).replace(/\\/g, "/")}` : path;
}
