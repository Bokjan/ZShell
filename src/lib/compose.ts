import { focusedPane, type Pane, type Tab } from "./panes";

/**
 * Where the compose bar (and quick commands, while it is open) sends to: the focused pane,
 * the panes of the active tab, all panes, or the selected ones.
 */
export type ComposeScope = "current" | "tab" | "all" | "selected";

export interface Compose {
  open: boolean;
  scope: ComposeScope;
  /** Pane keys, for the "selected" scope. */
  selected: number[];
  /** Typing in the focused terminal goes to the other panes in scope too. */
  sync: boolean;
}

/** What happened to text sent to the panes in scope. */
export interface SendResult {
  /** Titles of the panes it was sent to. */
  sent: string[];
  /** Panes in scope that aren't connected. */
  skipped: number;
}

export const CLOSED_COMPOSE: Compose = { open: false, scope: "current", selected: [], sync: false };

/**
 * The panes that text sent from the compose bar or a quick command goes to: the bar's scope
 * while it is open, otherwise the active tab's focused pane only (so a scope left on "all
 * tabs" can't send anywhere unnoticed once the bar is closed).
 */
export function scopePanes(compose: Compose, tabs: Tab[], activeKey: number | null): Pane[] {
  const active = tabs.find((tab) => tab.key === activeKey);
  if (!compose.open || compose.scope === "current") return active ? [focusedPane(active)] : [];
  if (compose.scope === "tab") return active?.panes ?? [];
  const panes = tabs.flatMap((tab) => tab.panes);
  if (compose.scope === "all") return panes;
  return panes.filter((pane) => compose.selected.includes(pane.key));
}

/** Whether sending goes beyond the focused pane, which the UI makes prominent. */
export const sendsToMany = (compose: Compose) => compose.open && compose.scope !== "current";

export const isConnected = (pane: Pane) => pane.status === "connected" && pane.sessionId != null;

/** Text as if typed: line breaks become Enter, and `enter` presses it once more at the end. */
export const asTyped = (text: string, enter: boolean) => text.replace(/\r?\n/g, "\r") + (enter ? "\r" : "");
