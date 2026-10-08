import type { Folder, Profile } from "./api";

/** A visible line of the session tree. */
export type Row =
  | { kind: "folder"; folder: Folder; depth: number; expanded: boolean; count: number }
  | { kind: "profile"; profile: Profile; depth: number };

export const rowKey = (row: Row) => (row.kind === "folder" ? `f:${row.folder.id}` : `p:${row.profile.id}`);

/** `user@host`, with the port if it isn't 22. */
export const address = (p: { username: string; host: string; port: number }) => {
  const host = p.host.includes(":") ? `[${p.host}]` : p.host;
  return `${p.username ? `${p.username}@` : ""}${host}${p.port !== 22 ? `:${p.port}` : ""}`;
};

/**
 * The tree's visible rows: in each folder its subfolders, then its sessions, in saved order;
 * a collapsed folder hides what it contains.
 */
export function treeRows(folders: Folder[], profiles: Profile[], collapsed: Set<string>): Row[] {
  const rows: Row[] = [];
  const visit = (parent: string | undefined, depth: number) => {
    for (const folder of folders.filter((f) => f.parent === parent)) {
      const expanded = !collapsed.has(folder.id);
      rows.push({ kind: "folder", folder, depth, expanded, count: sessionsIn(folder.id, folders, profiles).length });
      if (expanded) visit(folder.id, depth + 1);
    }
    for (const profile of profiles.filter((p) => p.folder === parent)) rows.push({ kind: "profile", profile, depth });
  };
  visit(undefined, 0);
  return rows;
}

/** The sessions in a folder and its subfolders, in tree order. */
export function sessionsIn(folderId: string, folders: Folder[], profiles: Profile[]): Profile[] {
  const result: Profile[] = [];
  const visit = (id: string) => {
    for (const sub of folders.filter((f) => f.parent === id)) visit(sub.id);
    result.push(...profiles.filter((p) => p.folder === id));
  };
  visit(folderId);
  return result;
}

/** Whether `folder` is `ancestor` or inside it. */
export function isWithin(folder: string | undefined, ancestor: string, folders: Folder[]): boolean {
  for (let id = folder, steps = 0; id && steps <= folders.length; steps++) {
    if (id === ancestor) return true;
    id = folders.find((f) => f.id === id)?.parent;
  }
  return false;
}

/** Folder names from the top level down to `folderId`. */
export function folderPath(folderId: string | undefined, folders: Folder[]): string[] {
  const path: string[] = [];
  for (let id = folderId; id && path.length <= folders.length; ) {
    const folder = folders.find((f) => f.id === id);
    if (!folder) break;
    path.unshift(folder.name);
    id = folder.parent;
  }
  return path;
}

/** Sessions whose name, address or user name contain every word of `query`, in tree order. */
export function search(query: string, folders: Folder[], profiles: Profile[]): Profile[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const ordered = treeRows(folders, profiles, new Set()).flatMap((row) => (row.kind === "profile" ? [row.profile] : []));
  return ordered.filter((p) => {
    const text = `${p.name} ${address(p)} ${p.username} ${p.host}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

export interface QuickTarget {
  /** Empty for the local user name, as with `ssh host`. */
  username: string;
  host: string;
  port: number;
}

/**
 * `[ssh ][user@]host[:port]` (IPv6 in brackets) as a quick connection. Plain words without a
 * user, a dot or a port are left to the search (they are more likely session names).
 */
export function parseQuickConnect(text: string): QuickTarget | null {
  const match = /^(?:ssh\s+)?(?:([^\s@]+)@)?(\[[0-9a-fA-F:.]+\]|[^\s:@[\]]+)(?::(\d{1,5}))?$/.exec(text.trim());
  if (!match) return null;
  const [, username = "", rawHost, rawPort] = match;
  const host = rawHost.replace(/^\[|\]$/g, "");
  const port = rawPort ? Number(rawPort) : 22;
  if (port < 1 || port > 65535) return null;
  const looksLikeHost = username !== "" || rawPort !== undefined || host.includes(".") || host.includes(":");
  return looksLikeHost ? { username, host, port } : null;
}

const RECENT_KEY = "zshell.recentSessions";
const COLLAPSED_KEY = "zshell.collapsedFolders";
export const RECENT_LIMIT = 5;

// Layout and history, kept per machine like the sidebar width rather than in the settings.
function readList(key: string): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
  } catch {
    return [];
  }
}

function writeList(key: string, list: string[]) {
  try {
    localStorage.setItem(key, JSON.stringify(list));
  } catch {
    // Not persisted; still applies until the app restarts.
  }
}

export const storedRecent = () => readList(RECENT_KEY);

/** Puts `id` first in the recent sessions; returns the new list. */
export function addRecent(id: string): string[] {
  const list = [id, ...readList(RECENT_KEY).filter((r) => r !== id)].slice(0, RECENT_LIMIT * 2);
  writeList(RECENT_KEY, list);
  return list;
}

export const storedCollapsed = () => new Set(readList(COLLAPSED_KEY));
export const storeCollapsed = (collapsed: Set<string>) => writeList(COLLAPSED_KEY, [...collapsed]);
