import type { FileEntry } from "./api";

export type SortKey = "name" | "size" | "modified";

export interface FileView {
  showHidden: boolean;
  sort: SortKey;
  descending: boolean;
}

const VIEW_KEY = "zshell.fileView";
const DEFAULT_VIEW: FileView = { showHidden: true, sort: "name", descending: false };

/** How the file panel lists files; layout state, kept per machine like the sidebar width. */
export function storedView(): FileView {
  try {
    return { ...DEFAULT_VIEW, ...JSON.parse(localStorage.getItem(VIEW_KEY) ?? "{}") };
  } catch {
    return DEFAULT_VIEW;
  }
}

export function storeView(view: FileView) {
  try {
    localStorage.setItem(VIEW_KEY, JSON.stringify(view));
  } catch {
    // Not persisted; still applies until the app restarts.
  }
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/**
 * The entries shown: hidden files (dotfiles) only when asked for, names containing every word
 * of `filter` (ignoring case), folders first, then by the sort key with name as tiebreaker.
 */
export function visibleEntries(entries: FileEntry[], view: FileView, filter: string): FileEntry[] {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  const byName = (a: FileEntry, b: FileEntry) => collator.compare(a.name, b.name);
  const byKey = (a: FileEntry, b: FileEntry) => {
    // Folders have no meaningful size, so they stay in name order when sorting by size.
    if (view.sort === "size" && !a.isDir) return a.size - b.size;
    if (view.sort === "modified") return (a.modified ?? 0) - (b.modified ?? 0);
    return 0;
  };
  const direction = view.descending ? -1 : 1;
  return entries
    .filter((entry) => view.showHidden || !entry.name.startsWith("."))
    .filter((entry) => words.every((word) => entry.name.toLowerCase().includes(word)))
    .sort((a, b) => Number(b.isDir) - Number(a.isDir) || direction * (byKey(a, b) || byName(a, b)));
}

/** The paths from `from` to `to` (in either direction) in the listed order. */
export function pathRange(entries: FileEntry[], from: string, to: string): string[] {
  const a = entries.findIndex((entry) => entry.path === from);
  const b = entries.findIndex((entry) => entry.path === to);
  if (a < 0 || b < 0) return [to];
  return entries.slice(Math.min(a, b), Math.max(a, b) + 1).map((entry) => entry.path);
}
