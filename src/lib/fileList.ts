import type { FileEntry } from "../ipc/api";
import { storedJson, storeJson } from "./storage";

export type SortKey = "name" | "size" | "modified";

export interface FileView {
  showHidden: boolean;
  /** Whether the panel goes to each folder its terminal's shell goes to. */
  followTerminal: boolean;
  sort: SortKey;
  descending: boolean;
}

const VIEW_KEY = "zshell.fileView";
const DEFAULT_VIEW: FileView = { showHidden: true, followTerminal: false, sort: "name", descending: false };

/** How the file panel lists files; layout state, kept per machine like the sidebar width. */
export function storedView(): FileView {
  const stored = storedJson(VIEW_KEY);
  return typeof stored === "object" && stored !== null ? { ...DEFAULT_VIEW, ...stored } : DEFAULT_VIEW;
}

type Compare = (a: string, b: string) => number;

export const storeView = (view: FileView) => storeJson(VIEW_KEY, view);

/**
 * The entries shown: hidden files (dotfiles) only when asked for, names containing every word
 * of `filter` (ignoring case), folders first, then by the sort key with name (in `compare`'s
 * order: `compareNames`) as tiebreaker.
 */
export const visibleEntries = (entries: FileEntry[], view: FileView, filter: string, compare: Compare): FileEntry[] =>
  filterEntries(sortedEntries(entries, view, compare), filter);

/** The entries shown before filtering by name (see `visibleEntries`), in order. */
export function sortedEntries(entries: FileEntry[], view: FileView, compare: Compare): FileEntry[] {
  const byName = (a: FileEntry, b: FileEntry) => compare(a.name, b.name);
  const byKey = (a: FileEntry, b: FileEntry) => {
    // Folders have no meaningful size, so they stay in name order when sorting by size.
    if (view.sort === "size" && !a.isDir) return a.size - b.size;
    if (view.sort === "modified") return (a.modified ?? 0) - (b.modified ?? 0);
    return 0;
  };
  const direction = view.descending ? -1 : 1;
  return entries
    .filter((entry) => view.showHidden || !entry.name.startsWith("."))
    .sort((a, b) => Number(b.isDir) - Number(a.isDir) || direction * (byKey(a, b) || byName(a, b)));
}

/** Those of `entries` whose names contain every word of `filter`, ignoring case, in order. */
export function filterEntries(entries: FileEntry[], filter: string): FileEntry[] {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return entries;
  return entries.filter((entry) => {
    const name = entry.name.toLowerCase();
    return words.every((word) => name.includes(word));
  });
}

/** A permission mode as typed (`644`, `0755`, octal), or null if it isn't one. */
export const parseMode = (text: string): number | null => (/^[0-7]{3,4}$/.test(text.trim()) ? parseInt(text.trim(), 8) : null);
