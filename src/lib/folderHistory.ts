import { isKey } from "./keys";

/**
 * The folders a file panel has shown, for going back and forward as in a file manager: the
 * paths in the order shown, and which of them is shown now.
 */
export interface FolderHistory {
  readonly paths: readonly string[];
  /** The shown folder's place in `paths`; -1 before any is shown. */
  readonly index: number;
}

export const NO_HISTORY: FolderHistory = { paths: [], index: -1 };

/** How many folders are kept; the oldest go first. */
export const HISTORY_LIMIT = 50;

/**
 * `path` was shown after the current folder, other than by going back or forward: the folders
 * ahead are dropped, as in a browser. The current folder shown again changes nothing.
 */
export function visited(history: FolderHistory, path: string): FolderHistory {
  if (history.paths[history.index] === path) return history;
  const paths = [...history.paths.slice(0, history.index + 1), path].slice(-HISTORY_LIMIT);
  return { paths, index: paths.length - 1 };
}

/** The folder `step` places away (-1 back, 1 forward) from `index`, if there is one. */
export function stepFrom(history: FolderHistory, index: number, step: -1 | 1): { index: number; path: string } | null {
  const target = index + step;
  return target >= 0 && target < history.paths.length ? { index: target, path: history.paths[target] } : null;
}

/**
 * Gone back or forward to the folder at `index`, shown as `path`: listed at another path than
 * the one kept (a link the server now resolves elsewhere), it takes that one's place.
 */
export function wentTo(history: FolderHistory, index: number, path: string): FolderHistory {
  if (index < 0 || index >= history.paths.length) return history;
  const paths = history.paths[index] === path ? history.paths : history.paths.map((p, i) => (i === index ? path : p));
  return { paths, index };
}

/** What of a key press `historyKey` reads. */
export interface HistoryKey {
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * Back (-1) or forward (1) by a key: Cmd+[ and Cmd+] on macOS, as in Finder, and Alt+Left and
 * Alt+Right elsewhere, as in Explorer. The brackets go by the character, as the app's shortcuts
 * do (`isKey`). Option is allowed, which some layouts need for them (German).
 */
export function historyKey(e: HistoryKey, mac: boolean): -1 | 1 | null {
  if (e.shiftKey || e.ctrlKey) return null;
  if (!mac) {
    if (!e.altKey || e.metaKey) return null;
    return e.code === "ArrowLeft" ? -1 : e.code === "ArrowRight" ? 1 : null;
  }
  if (!e.metaKey) return null;
  return isKey(e, "[") ? -1 : isKey(e, "]") ? 1 : null;
}

/** The keys of `historyKey` as tooltips write them: "⌘[", or "Alt+←" with `alt` the Alt key's name. */
export const historyKeyLabel = (step: -1 | 1, mac: boolean, alt: string): string =>
  mac ? (step < 0 ? "⌘[" : "⌘]") : `${alt}+${step < 0 ? "←" : "→"}`;
