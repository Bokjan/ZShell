import { describe, expect, it } from "vitest";

import type { FileEntry } from "./api";
import { parseMode, visibleEntries, type FileView } from "./fileList";

// Only what the list looks at.
const file = (name: string, size = 0, modified: number | null = null) => ({ name, isDir: false, size, modified }) as FileEntry;
const dir = (name: string, modified: number | null = null) => ({ name, isDir: true, size: 0, modified }) as FileEntry;
const view = (changes: Partial<FileView> = {}): FileView => ({
  showHidden: true,
  followTerminal: false,
  sort: "name",
  descending: false,
  ...changes,
});
const names = (entries: FileEntry[]) => entries.map((entry) => entry.name);

describe("visibleEntries", () => {
  const entries = [file("file10", 30, 300), file("file2", 10, null), dir("Docs", 200), dir("a", 100), file(".bashrc", 20, 100)];

  it("lists folders first, then by name with numbers in order", () => {
    expect(names(visibleEntries(entries, view(), ""))).toEqual(["a", "Docs", ".bashrc", "file2", "file10"]);
    expect(names(visibleEntries(entries, view({ descending: true }), ""))).toEqual(["Docs", "a", "file10", "file2", ".bashrc"]);
  });

  it("hides dotfiles only when asked to", () => {
    expect(names(visibleEntries(entries, view({ showHidden: false }), ""))).toEqual(["a", "Docs", "file2", "file10"]);
  });

  it("sorts files by size and keeps folders in name order", () => {
    expect(names(visibleEntries(entries, view({ sort: "size" }), ""))).toEqual(["a", "Docs", "file2", ".bashrc", "file10"]);
  });

  it("sorts by modification time, unknown first, and by name on a tie", () => {
    expect(names(visibleEntries(entries, view({ sort: "modified" }), ""))).toEqual(["a", "Docs", "file2", ".bashrc", "file10"]);
    expect(names(visibleEntries([file("b", 0, 5), file("a", 0, 5)], view({ sort: "modified" }), ""))).toEqual(["a", "b"]);
  });

  it("keeps names containing every word of the filter, ignoring case", () => {
    const logs = [file("app.log"), file("app-old.LOG"), file("old.txt")];
    expect(names(visibleEntries(logs, view(), "  LOG  old "))).toEqual(["app-old.LOG"]);
    expect(names(visibleEntries(logs, view(), ""))).toHaveLength(3);
  });
});

describe("parseMode", () => {
  it("reads three or four octal digits", () => {
    expect(parseMode("644")).toBe(0o644);
    expect(parseMode(" 0755 ")).toBe(0o755);
    expect(parseMode("4755")).toBe(0o4755);
  });

  it("refuses anything else", () => {
    for (const text of ["", "75", "12345", "800", "rwx", "0o644"]) expect(parseMode(text)).toBeNull();
  });
});
