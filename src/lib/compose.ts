import type { Tab } from "../components/TabBar";

/** Where the compose bar (and quick commands, while it is open) sends to. */
export type ComposeScope = "current" | "all" | "selected";

export interface Compose {
  open: boolean;
  scope: ComposeScope;
  /** Tab keys, for the "selected" scope. */
  selected: number[];
  /** Typing in the active terminal goes to the other tabs in scope too. */
  sync: boolean;
}

/** What happened to text sent to the tabs in scope. */
export interface SendResult {
  /** Titles of the tabs it was sent to. */
  sent: string[];
  /** Tabs in scope that aren't connected. */
  skipped: number;
}

export const CLOSED_COMPOSE: Compose = { open: false, scope: "current", selected: [], sync: false };

/**
 * The tabs that text sent from the compose bar or a quick command goes to: the bar's scope
 * while it is open, otherwise the active tab only (so a scope left on "all tabs" can't send
 * anywhere unnoticed once the bar is closed).
 */
export function scopeTabs(compose: Compose, tabs: Tab[], activeKey: number | null): Tab[] {
  if (!compose.open || compose.scope === "current") return tabs.filter((tab) => tab.key === activeKey);
  if (compose.scope === "all") return tabs;
  return tabs.filter((tab) => compose.selected.includes(tab.key));
}

/** Whether sending goes beyond the active tab, which the UI makes prominent. */
export const sendsToMany = (compose: Compose) => compose.open && compose.scope !== "current";

export const isConnected = (tab: Tab) => tab.status === "connected" && tab.sessionId != null;

/** Text as if typed: line breaks become Enter, and `enter` presses it once more at the end. */
export const asTyped = (text: string, enter: boolean) => text.replace(/\r?\n/g, "\r") + (enter ? "\r" : "");
