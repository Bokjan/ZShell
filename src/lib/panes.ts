import type { SessionStatus } from "../components/TerminalView";
import type { ForwardState, Protocol, SessionId, SessionTarget } from "./api";

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
  /** The backend session, while there is one. */
  sessionId: SessionId | null;
  /** For a duplicated SSH pane: the session whose connection its first shell runs on. */
  shareFrom?: SessionId;
  /** Incremented to close the session and connect again. */
  reconnectKey: number;
  /** Live state of the profile's forwarding rules on this pane's connection, by rule id. */
  forwards: Record<string, ForwardState>;
  /** The quick command group picked in this pane; null shows its session's. */
  commandGroup: string | null;
  /** The log this pane writes, kept across reconnections (which go on with it). */
  logPath: string | null;
  /** Logging was stopped by hand: reconnections don't start it again. */
  logStopped: boolean;
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

export const focusedPane = (tab: Tab) => tab.panes.find((pane) => pane.key === tab.focused) ?? tab.panes[0];

/** Whether the pane is writing its log now. */
export const isLogging = (pane: Pane) => pane.logPath !== null && pane.status !== "closed";

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
