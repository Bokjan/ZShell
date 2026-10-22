import { DEFAULT_PORTS, type Folder, type Profile, type SerialOptions, type TreeItem } from "../ipc/api";
import { storedJson, storeJson } from "./storage";

/** A visible line of the session tree. */
export type Row =
  | { kind: "folder"; folder: Folder; depth: number; expanded: boolean; count: number }
  | { kind: "profile"; profile: Profile; depth: number };

export const rowKey = (row: Row) => (row.kind === "folder" ? `f:${row.folder.id}` : `p:${row.profile.id}`);

/** What `address` needs: the connection of a session (or an import candidate), or a quick connection. */
export type Addressable =
  { protocol: "ssh" | "telnet"; username: string; host: string; port: number } | ({ protocol: "serial" } & SerialOptions);

/** The usual short form of serial settings, `115200 8N1`. */
export const serialSummary = (s: SerialOptions) =>
  `${s.baudRate} ${s.dataBits}${s.parity === "none" ? "N" : s.parity === "odd" ? "O" : "E"}${s.stopBits}`;

/**
 * Where a session connects, as shown in the session list: SSH as `user@host` (with the port
 * if it isn't 22), Telnet as `telnet://[user@]host[:port]`, serial as `COM3 · 115200 8N1`.
 */
export const address = (c: Addressable) => {
  if (c.protocol === "serial") return `${c.device} · ${serialSummary(c)}`;
  const host = c.host.includes(":") ? `[${c.host}]` : c.host;
  const where = `${c.username ? `${c.username}@` : ""}${host}${c.port !== DEFAULT_PORTS[c.protocol] ? `:${c.port}` : ""}`;
  return c.protocol === "telnet" ? `telnet://${where}` : where;
};

/** The saved sessions and folders, and the order they are shown in (`DirectoryState` is one). */
export interface SessionTree {
  folders: Folder[];
  profiles: Profile[];
  order: TreeItem[];
}

/** A session or a folder in the tree. */
export type Child = { kind: "folder"; folder: Folder } | { kind: "profile"; profile: Profile };

export const childItem = (child: Child): TreeItem =>
  child.kind === "folder" ? { kind: "folder", id: child.folder.id } : { kind: "profile", id: child.profile.id };

/** The key of the item's row (see `rowKey`). */
export const itemKey = (item: TreeItem) => `${item.kind === "folder" ? "f" : "p"}:${item.id}`;

/**
 * What each folder holds (`undefined`: the top level), sessions and subfolders mixed in the
 * order shown. Those the order leaves out (read in between two changes) come after the
 * others, folders first. A session whose folder is missing (its file was set aside) is at
 * the top level.
 */
export function childrenByFolder(tree: SessionTree): Map<string | undefined, Child[]> {
  const rank = new Map<string, number>();
  for (const item of tree.order) if (!rank.has(itemKey(item))) rank.set(itemKey(item), rank.size);
  const unranked = (offset: number) => tree.order.length + offset;
  const ids = new Set(tree.folders.map((f) => f.id));
  const ranked: { child: Child; parent: string | undefined; rank: number }[] = [
    ...tree.folders.map((folder, i) => ({
      child: { kind: "folder", folder } as Child,
      parent: folder.parent,
      rank: rank.get(`f:${folder.id}`) ?? unranked(i),
    })),
    ...tree.profiles.map((profile, i) => ({
      child: { kind: "profile", profile } as Child,
      parent: profile.folder !== undefined && ids.has(profile.folder) ? profile.folder : undefined,
      rank: rank.get(`p:${profile.id}`) ?? unranked(tree.folders.length + i),
    })),
  ];
  ranked.sort((a, b) => a.rank - b.rank);
  const children = new Map<string | undefined, Child[]>();
  for (const { child, parent } of ranked) {
    const list = children.get(parent);
    if (list) list.push(child);
    else children.set(parent, [child]);
  }
  return children;
}

/** The tree's visible rows, in the order shown; a collapsed folder hides what it contains. */
export function treeRows(tree: SessionTree, collapsed: Set<string>): Row[] {
  const children = childrenByFolder(tree);
  const rows: Row[] = [];
  const visit = (parent: string | undefined, depth: number) => {
    for (const child of children.get(parent) ?? []) {
      if (child.kind === "profile") {
        rows.push({ kind: "profile", profile: child.profile, depth });
        continue;
      }
      const { folder } = child;
      const expanded = !collapsed.has(folder.id);
      rows.push({ kind: "folder", folder, depth, expanded, count: sessionsUnder(children, folder.id).length });
      if (expanded) visit(folder.id, depth + 1);
    }
  };
  visit(undefined, 0);
  return rows;
}

/** The sessions in a folder and its subfolders, in the order shown. */
export function sessionsIn(folderId: string, tree: SessionTree): Profile[] {
  return sessionsUnder(childrenByFolder(tree), folderId);
}

function sessionsUnder(children: Map<string | undefined, Child[]>, folderId: string): Profile[] {
  const result: Profile[] = [];
  // Bounded, in case of a cycle (the backend breaks them when it loads the folders).
  const visit = (id: string, depth: number) => {
    if (depth > children.size) return;
    for (const child of children.get(id) ?? []) {
      if (child.kind === "profile") result.push(child.profile);
      else visit(child.folder.id, depth + 1);
    }
  };
  visit(folderId, 0);
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
  for (let id = folderId; id && path.length <= folders.length;) {
    const folder = folders.find((f) => f.id === id);
    if (!folder) break;
    path.unshift(folder.name);
    id = folder.parent;
  }
  return path;
}

/**
 * Sessions whose name, address or user name contain every word of `query`, in tree order; then
 * those found only with their notes, which match more loosely.
 */
export function search(query: string, tree: SessionTree): Profile[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const ordered = treeRows(tree, new Set()).flatMap((row) => (row.kind === "profile" ? [row.profile] : []));
  const matches = (text: string) => {
    const lower = text.toLowerCase();
    return words.every((word) => lower.includes(word));
  };
  const target = (p: Profile) => {
    const c = p.connection;
    return `${p.name} ${address(c)} ${c.protocol === "serial" ? "" : `${c.username} ${c.host}`}`;
  };
  const byTarget = ordered.filter((p) => matches(target(p)));
  const found = new Set(byTarget);
  const byNotes = ordered.filter((p) => !found.has(p) && p.notes && matches(`${target(p)} ${p.notes}`));
  return [...byTarget, ...byNotes];
}

/** Lines of a session's notes shown where it is listed, and characters. */
const PREVIEW_LINES = 8;
const PREVIEW_CHARS = 400;

/** The start of a session's notes, for its tooltip: cut at a line or character limit, marked with "…". */
export function notesPreview(notes: string): string {
  let preview = notes.split("\n").slice(0, PREVIEW_LINES).join("\n");
  if (preview.length > PREVIEW_CHARS) preview = preview.slice(0, PREVIEW_CHARS);
  return preview.length < notes.length ? `${preview.trimEnd()}…` : notes;
}

export interface QuickTarget {
  protocol: "ssh" | "telnet";
  /** Empty for SSH's local user name (as with `ssh host`), or for no Telnet login. */
  username: string;
  host: string;
  port: number;
}

/** The user a quick connection logs in as: SSH without a user name logs in as the local user, as `ssh host` does. */
export const loginName = (target: { protocol: string; username: string }, localUser: string) =>
  target.username || (target.protocol === "ssh" ? localUser : "");

/**
 * `[ssh ][user@]host[:port]` (IPv6 in brackets) as a quick SSH connection, and
 * `telnet [user@]host[:port]` or `telnet://[user@]host[:port]` as a Telnet one. Plain words
 * without a user, a dot or a port are left to the search (they are more likely session
 * names), unless they say telnet.
 */
export function parseQuickConnect(text: string): QuickTarget | null {
  const match = /^(?:(ssh|telnet)\s+|(ssh|telnet):\/\/)?(?:([^\s@/]+)@)?(\[[0-9a-fA-F:.]+\]|[^\s:@/[\]]+)(?::(\d{1,5}))?\/?$/i.exec(
    text.trim(),
  );
  if (!match) return null;
  const [, word, scheme, username = "", rawHost, rawPort] = match;
  const protocol = (word ?? scheme ?? "ssh").toLowerCase() as QuickTarget["protocol"];
  const host = rawHost.replace(/^\[|\]$/g, "");
  const port = rawPort ? Number(rawPort) : DEFAULT_PORTS[protocol];
  if (port < 1 || port > 65535) return null;
  const looksLikeHost = protocol === "telnet" || username !== "" || rawPort !== undefined || host.includes(".") || host.includes(":");
  return looksLikeHost ? { protocol, username, host, port } : null;
}

/** Where the recent sessions are kept; the windows hear each other change it (`storage` events). */
export const RECENT_KEY = "zshell.recentSessions";
const COLLAPSED_KEY = "zshell.collapsedFolders";
export const RECENT_LIMIT = 5;

// Layout and history, kept per machine like the sidebar width rather than in the settings.
function readList(key: string): string[] {
  const value = storedJson(key);
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}

const writeList = (key: string, list: string[]) => storeJson(key, list);

export const storedRecent = () => readList(RECENT_KEY);

/**
 * `list` with `id` first, once. Twice as many as shown are kept, so that deleting a few
 * sessions still leaves the list full.
 */
export const withRecent = (list: string[], id: string) => [id, ...list.filter((r) => r !== id)].slice(0, RECENT_LIMIT * 2);

/** Puts `id` first in the recent sessions, as stored now (another window may have added one); returns the new list. */
export function addRecent(id: string): string[] {
  const list = withRecent(readList(RECENT_KEY), id);
  writeList(RECENT_KEY, list);
  return list;
}

export const storedCollapsed = () => new Set(readList(COLLAPSED_KEY));
export const storeCollapsed = (collapsed: Set<string>) => writeList(COLLAPSED_KEY, [...collapsed]);
