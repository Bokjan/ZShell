import {
  DEFAULT_TERM_TYPE,
  type ForwardState,
  type Profile,
  type Protocol,
  type SessionId,
  type SessionSnapshot,
  type SessionTarget,
} from "./api";
import type { SessionStatus } from "./paneSession";

export type SidePanel = "files" | "forwards";

/** What a pane runs: a remote session's protocol, or a local terminal. */
export type TabProtocol = Protocol | "local";

/** One terminal in a tab, with a session of its own. */
export interface Pane {
  key: number;
  target: SessionTarget;
  /** Of the current (or last) session; a saved session's may change between connections. */
  protocol: TabProtocol;
  /** The session name: the profile's, or the local shell's. */
  title: string;
  /** Set by the shell (OSC 0 / 2); shown unless turned off in the settings. */
  remoteTitle: string | null;
  status: SessionStatus;
  /** The terminal type (`TERM`) the current (or last) session started with; none for serial ports. */
  termType: string | null;
  /** When the next automatic reconnection is due (`Date.now()`), while one is. */
  retryAt: number | null;
  /** The backend session, while there is one. */
  sessionId: SessionId | null;
  /** For a duplicated SSH pane: the session whose connection its first shell runs on. */
  shareFrom?: SessionId;
  /** Live state of the profile's forwarding rules on this pane's connection, by rule id. */
  forwards: Record<string, ForwardState>;
  /** The quick command group picked in this pane; null shows its session's. */
  commandGroup: string | null;
  /** The log this pane writes, kept across reconnections (which go on with it). */
  logPath: string | null;
  /** Logging was stopped by hand: reconnections don't start it again. */
  logStopped: boolean;
  /** Uploads and downloads running in the pane's file panel; closing the pane cancels them. */
  transfers: number;
  /** Files of the pane's file panel open in a local editor, which closing the pane stops watching. */
  edits: number;
  /**
   * Questions waiting in the pane's file panel (an edit conflict a background save found),
   * marked on its tab while the panel is out of sight.
   */
  questions: number;
  /** For a pane that came from another window, until its terminal takes over. */
  arrival?: Arrival;
}

/** A terminal's content, to draw again in another window (see `SessionSnapshot`). */
export type Snapshot = Omit<SessionSnapshot, "id" | "moveId">;

/**
 * How a pane that moved from another window goes on: with its session (`sessionId`), whose
 * screen the old window hands over once the session has moved, or without one, its terminal
 * as it was (`snapshot`).
 */
export interface Arrival {
  sessionId: SessionId | null;
  /** The session's shell is up (rather than still connecting). */
  connected: boolean;
  snapshot: Snapshot | null;
  /** It was waiting to reconnect: it connects at once. */
  reconnect: boolean;
  /** The window it came from, and its key there, to tell if its session can't move. */
  from: { window: string; pane: number };
}

/** How a tab's panes are arranged: one pane, or several side by side (`row`) or stacked (`column`). */
export type Layout =
  | { kind: "pane"; key: number }
  | { kind: "split"; direction: "row" | "column"; children: Layout[]; sizes: number[] };

export interface Tab {
  key: number;
  layout: Layout;
  /** In the order they were opened, which keeps their terminals mounted as the layout changes. */
  panes: Pane[];
  /** The pane with the keyboard focus: the tab shows its title and status, and its side panel. */
  focused: number;
  /** Set by renaming the tab; shown instead of any other title. */
  customTitle: string | null;
  /** The side panel shown next to the panes, if any; it shows the focused pane's connection. */
  sidePanel: SidePanel | null;
}

/** What a pane's next session will be: a saved session's protocol may have been changed. */
export function targetProtocol(target: SessionTarget, profiles: Profile[]): TabProtocol {
  if (target.kind === "local") return "local";
  if (target.kind === "quick") return target.protocol;
  return profiles.find((p) => p.id === target.profileId)?.connection.protocol ?? "ssh";
}

/**
 * The terminal type a session of the target starts with: the saved session's, the default
 * for quick connections, and the one local shells get (see `pty/shell.rs`).
 */
export function targetTermType(target: SessionTarget, profiles: Profile[]): string | null {
  if (target.kind !== "profile") return DEFAULT_TERM_TYPE;
  const connection = profiles.find((p) => p.id === target.profileId)?.connection;
  if (!connection) return DEFAULT_TERM_TYPE;
  return connection.protocol === "serial" ? null : connection.termType;
}

/** Ids that connect a tab and its page for screen readers. */
export const tabId = (key: number) => `tab-${key}`;
export const tabPanelId = (key: number) => `tab-panel-${key}`;

export const focusedPane = (tab: Tab) => tab.panes.find((pane) => pane.key === tab.focused) ?? tab.panes[0];

/** Whether the pane is writing its log now. */
export const isLogging = (pane: Pane) => pane.logPath !== null && pane.status !== "closed";

/**
 * What the pane's file panel is doing that closing the pane would end, connected or not, and
 * that can't move to another window with it: transfers (cancelled), files open in a local
 * editor (their saves are no longer uploaded, including those waiting for a reconnection),
 * questions waiting (an edit conflict). Closing and moving both ask this.
 */
export type PaneWork = { kind: "transfers"; count: number } | { kind: "edits"; count: number } | { kind: "questions"; count: number };

export function paneWork(pane: Pane): PaneWork | null {
  if (pane.transfers > 0) return { kind: "transfers", count: pane.transfers };
  if (pane.edits > 0) return { kind: "edits", count: pane.edits };
  if (pane.questions > 0) return { kind: "questions", count: pane.questions };
  return null;
}

export const paneTitle = (pane: Pane, followRemoteTitle: boolean) => (followRemoteTitle && pane.remoteTitle) || pane.title;

export const tabTitle = (tab: Tab, followRemoteTitle: boolean) =>
  tab.customTitle ?? paneTitle(focusedPane(tab), followRemoteTitle);

/** What a pane is called where panes are listed: its tab's title while it is the tab's only one. */
export const paneLabel = (tab: Tab, pane: Pane, followRemoteTitle: boolean) =>
  tab.panes.length === 1 ? tabTitle(tab, followRemoteTitle) : paneTitle(pane, followRemoteTitle);

/** The tab a pane is in, and the pane. */
export function findPane(tabs: Tab[], key: number): { tab: Tab; pane: Pane } | null {
  for (const tab of tabs) {
    const pane = tab.panes.find((p) => p.key === key);
    if (pane) return { tab, pane };
  }
  return null;
}
