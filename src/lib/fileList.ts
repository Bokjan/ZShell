import type { FileEntry } from "./api";
import { storedJson, storeJson } from "./storage";

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
  const stored = storedJson(VIEW_KEY);
  return typeof stored === "object" && stored !== null ? { ...DEFAULT_VIEW, ...stored } : DEFAULT_VIEW;
}

export const storeView = (view: FileView) => storeJson(VIEW_KEY, view);

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

/** A permission mode as typed (`644`, `0755`, octal), or null if it isn't one. */
export const parseMode = (text: string): number | null => (/^[0-7]{3,4}$/.test(text.trim()) ? parseInt(text.trim(), 8) : null);
