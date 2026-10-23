import {
  DEFAULT_TERM_TYPE,
  type ForwardState,
  type Profile,
  type Protocol,
  type SessionId,
  type SessionSnapshot,
  type SessionTarget,
  type TaskProgress,
  type TaskState,
} from "../ipc/api";
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
  /**
   * While the shell has exited and the session waits for the connections its port forwarding
   * still carries to end: how many.
   */
  waiting: number | null;
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
  /**
   * The progress a program in the pane reports (OSC 9;4), while it does. Not taken to another
   * window, whose session for the pane starts without it.
   */
  progress: TaskProgress | null;
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
export type Layout = { kind: "pane"; key: number } | { kind: "split"; direction: "row" | "column"; children: Layout[]; sizes: number[] };

/** Which of several progresses shows: what most needs the user's eyes first. */
const PROGRESS_RANK: Record<TaskState, number> = { error: 3, paused: 2, normal: 1, indeterminate: 0 };

/**
 * The progress shown for several (a tab's panes, a window's tabs): the state that ranks
 * highest, and of several in progress the one furthest from done; null for none. Kept the same
 * as `merge` in `window/progress.rs`, which merges the windows'.
 */
export function mergeProgress(all: (TaskProgress | null)[]): TaskProgress | null {
  return all.reduce<TaskProgress | null>((merged, progress) => {
    if (!progress || !merged) return merged ?? progress;
    if (PROGRESS_RANK[progress.state] !== PROGRESS_RANK[merged.state])
      return PROGRESS_RANK[progress.state] > PROGRESS_RANK[merged.state] ? progress : merged;
    return { state: merged.state, percent: Math.min(merged.percent, progress.percent) };
  }, null);
}

/**
 * Why a tab in the background asks to be looked at, until it is shown: new output, or (which
 * outranks it) a bell or a program's notification.
 */
export type Attention = "output" | "bell" | "notice";

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
  /** Something happened in a pane while the tab was in the background (a bell outranks output). */
  attention?: Attention;
  /** Moving out to another window (see `useWindows`): it neither moves again nor closes meanwhile. */
  moving?: boolean;
}

// The sessions of each list by id, made once per list: the app looks up every tab's session
// as it renders, several times a second while a program changes a title. A list is never
// changed in place (the directory replaces it), so its map stays right; it goes with it.
const byId = new WeakMap<Profile[], Map<string, Profile>>();

/** The saved session `target` connects to; undefined for other targets and for deleted sessions. */
export function targetProfile(target: SessionTarget, profiles: Profile[]): Profile | undefined {
  if (target.kind !== "profile") return undefined;
  let index = byId.get(profiles);
  if (!index) {
    index = new Map();
    // The first of a repeated id, as references go to (`persist::unique_ids`).
    for (const profile of profiles) if (!index.has(profile.id)) index.set(profile.id, profile);
    byId.set(profiles, index);
  }
  return index.get(target.profileId);
}

/** What a pane's next session will be: a saved session's protocol may have been changed. */
export function targetProtocol(target: SessionTarget, profiles: Profile[]): TabProtocol {
  if (target.kind === "local") return "local";
  if (target.kind === "quick") return target.protocol;
  return targetProfile(target, profiles)?.connection.protocol ?? "ssh";
}

/**
 * The terminal type a session of the target starts with: the saved session's, the default
 * for quick connections, and the one local shells get (see `pty/shell.rs`).
 */
export function targetTermType(target: SessionTarget, profiles: Profile[]): string | null {
  const connection = targetProfile(target, profiles)?.connection;
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

/**
 * Whether the pane closes now that its shell has exited (`exit`, Ctrl+D), as in Xshell,
 * whatever the exit status (bash exits with the last command's). Not when the user never
 * typed into the shell (the server ended the session at once: what it said stays in sight),
 * when the shell was ended by a signal (killed: its last output, "Killed" or the end of a
 * build, stays in sight), nor while closing would end work of the file panel. A local shell
 * reports one or the other; servers may report neither.
 */
export function closesOnExit(pane: Pane, status: number | null, signal: string | null, typed: boolean): boolean {
  if (!typed || signal !== null || paneWork(pane)) return false;
  return pane.target.kind !== "local" || status !== null;
}

/**
 * Whether the pane has a remote session, connected or connecting, or is waiting to reconnect:
 * such panes keep the system awake when the settings say so (`keep_awake` in the backend).
 * Local terminals have no connection to lose.
 */
export function keepsAwake(pane: Pane): boolean {
  return pane.target.kind !== "local" && (pane.status !== "closed" || pane.retryAt !== null);
}

export const paneTitle = (pane: Pane, followRemoteTitle: boolean) => (followRemoteTitle && pane.remoteTitle) || pane.title;

export const tabTitle = (tab: Tab, followRemoteTitle: boolean) => tab.customTitle ?? paneTitle(focusedPane(tab), followRemoteTitle);

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
