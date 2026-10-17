import type { Folder, TreeItem } from "../ipc/api";
import { childItem, childrenByFolder, isWithin, itemKey, rowKey, type Row, type SessionTree } from "./sessions";

/**
 * Dragging rows of the session tree: rows move before or after other rows, sessions and
 * folders mixed, into folders, and to the end of the top level below the last row. A folder
 * can't go into itself or its subfolders. Where the pointer is (which row, how far down it)
 * is the component's to find; what that means is here.
 */

/** Where a dragged row would land: relative to a row, or at the end of the top level. */
export type Drop = { key: string; position: "before" | "after" | "into" } | { key: null };

export function sameDrop(a: Drop | null, b: Drop | null): boolean {
  if (!a || !b) return a === b;
  if (a.key === null || b.key === null) return a.key === b.key;
  return a.key === b.key && a.position === b.position;
}

/** The folder the row `key` is in; undefined at the top level, also for a session whose folder is gone. */
export function parentOf(key: string, rows: Row[], folders: Folder[]): string | undefined {
  const row = rows.find((row) => rowKey(row) === key);
  if (row?.kind === "folder") return row.folder.parent;
  const folder = row?.profile.folder;
  return folder !== undefined && folders.some((f) => f.id === folder) ? folder : undefined;
}

/**
 * Where `item` would land over row `key`, `offset` down it (0 at its top, 1 at its bottom):
 * before or after a session, and before, into or after a folder; null where it can't go.
 */
export function dropOnRow(item: TreeItem, key: string, offset: number, rows: Row[], folders: Folder[]): Drop | null {
  if (key === itemKey(item)) return null;
  if (item.kind === "folder" && isWithin(parentOf(key, rows, folders), item.id, folders)) return null;
  if (!key.startsWith("f:")) return { key, position: offset < 0.5 ? "before" : "after" };
  const target = key.slice(2);
  if (item.kind === "folder" && isWithin(target, item.id, folders)) return null;
  // Right below an expanded folder are its own rows: "after" it means after all of them,
  // which a line there would not show.
  const index = rows.findIndex((row) => row.kind === "folder" && row.folder.id === target);
  const open = index >= 0 && (rows[index + 1]?.depth ?? 0) > rows[index].depth;
  return { key, position: offset < 0.25 ? "before" : offset > 0.75 && !open ? "after" : "into" };
}

/**
 * The move `drop` asks for (see `tree.move`): the folder `item` goes into (null: the top
 * level), and the row it goes before there (null: after the others).
 */
export function dropMove(item: TreeItem, drop: Drop, tree: SessionTree, rows: Row[]): { parent: string | null; before: TreeItem | null } {
  if (drop.key === null) return { parent: null, before: null };
  if (drop.position === "into") return { parent: drop.key.slice(2), before: null };
  // Before or after a row: in its folder, before it or the row that follows it there.
  const parent = parentOf(drop.key, rows, tree.folders);
  const peers = (childrenByFolder(tree).get(parent) ?? []).map(childItem).filter((peer) => itemKey(peer) !== itemKey(item));
  const at = peers.findIndex((peer) => itemKey(peer) === drop.key) + (drop.position === "after" ? 1 : 0);
  return { parent: parent ?? null, before: peers[at] ?? null };
}
