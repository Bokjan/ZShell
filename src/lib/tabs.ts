import { useSyncExternalStore } from "react";

import type { SessionId, SessionTarget } from "./api";
import { heirOf, removeFromLayout, splitLayout, type Direction } from "./layout";
import { focusedPane, type Pane, type SidePanel, type Tab, type TabProtocol } from "./panes";

/** The tabs of the window, and which one is shown. */
export interface TabsState {
  tabs: Tab[];
  activeKey: number | null;
  /** The next key for a tab or a pane; tabs and panes never share one. */
  nextKey: number;
}

export const INITIAL_TABS: TabsState = { tabs: [], activeKey: null, nextKey: 1 };

export const activeTabOf = (state: TabsState) => state.tabs.find((tab) => tab.key === state.activeKey);

/** What a new pane runs; its key comes from the state. */
export interface PaneSpec {
  target: SessionTarget;
  protocol: TabProtocol;
  title: string;
  /** For a copy of an SSH pane: the session whose connection its first shell runs on. */
  shareFrom?: SessionId;
}

/** A change to a pane: its fields, or a function of the pane and its tab giving them. */
export type PanePatch = Partial<Pane> | ((pane: Pane, tab: Tab) => Partial<Pane>);

export type TabsAction =
  /** Opens a tab with one pane and shows it; after the tab `after` if given, else last. */
  | { type: "open"; pane: PaneSpec; after?: number }
  /** Splits pane `key`, opening `pane` right of it (`row`) or below it (`column`), and focuses it. */
  | { type: "split"; key: number; direction: Direction; pane: PaneSpec }
  /** Closes panes, and the tabs left without any. */
  | { type: "close"; keys: number[] }
  | { type: "activate"; key: number | null }
  /** Shows a pane: activates its tab and focuses it there. */
  | { type: "focusPane"; key: number }
  | { type: "move"; key: number; index: number }
  | { type: "updateTab"; key: number; patch: Partial<Tab> }
  | { type: "updatePane"; key: number; patch: PanePatch }
  /** Opens or closes a side panel of the active tab; opens one only for an SSH pane. */
  | { type: "togglePanel"; panel: SidePanel }
  /**
   * A pane's session starts connecting: forwarding states and the remote title are reset,
   * and the protocol is the target's current one. The side panel closes when the focused
   * pane is no longer SSH.
   */
  | { type: "connecting"; key: number; protocol: TabProtocol };

const newPane = (key: number, spec: PaneSpec): Pane => ({
  key,
  ...spec,
  remoteTitle: null,
  status: "connecting",
  sessionId: null,
  forwards: {},
  commandGroup: null,
  logPath: null,
  logStopped: false,
  transfers: 0,
});

const mapTab = (state: TabsState, key: number, f: (tab: Tab) => Tab): TabsState => ({
  ...state,
  tabs: state.tabs.map((tab) => (tab.key === key ? f(tab) : tab)),
});

const tabOf = (tabs: Tab[], paneKey: number) => tabs.find((tab) => tab.panes.some((p) => p.key === paneKey));

/**
 * Closes the panes, and the tabs left without any. A pane's space goes to a neighbor, which
 * gets the focus if the closed pane had it (see `removeFromLayout`). When the active tab
 * closes, the next remaining one to the right becomes active, else the nearest to the left.
 */
function close(state: TabsState, keys: number[]): TabsState {
  const tabs = state.tabs.flatMap((tab) => {
    const closed = tab.panes.filter((p) => keys.includes(p.key));
    if (closed.length === tab.panes.length) return [];
    if (closed.length === 0) return [tab];
    let { layout, focused } = tab;
    for (const pane of closed) {
      if (focused === pane.key) focused = heirOf(layout, pane.key) ?? focused;
      layout = removeFromLayout(layout, pane.key)!;
    }
    return [{ ...tab, layout, focused, panes: tab.panes.filter((p) => !keys.includes(p.key)) }];
  });
  const gone = (tab: Tab) => !tabs.some((t) => t.key === tab.key);
  const active = state.tabs.findIndex((t) => t.key === state.activeKey);
  let activeKey = state.activeKey;
  if (active >= 0 && gone(state.tabs[active])) {
    activeKey = (state.tabs.slice(active).find((t) => !gone(t)) ?? tabs[tabs.length - 1])?.key ?? null;
  }
  return { ...state, tabs, activeKey };
}

export function reduceTabs(state: TabsState, action: TabsAction): TabsState {
  switch (action.type) {
    case "open": {
      const pane = newPane(state.nextKey, action.pane);
      const tab: Tab = {
        key: state.nextKey + 1,
        layout: { kind: "pane", key: pane.key },
        panes: [pane],
        focused: pane.key,
        customTitle: null,
        sidePanel: null,
      };
      const at = action.after === undefined ? -1 : state.tabs.findIndex((t) => t.key === action.after);
      const tabs = at < 0 ? [...state.tabs, tab] : [...state.tabs.slice(0, at + 1), tab, ...state.tabs.slice(at + 1)];
      return { tabs, activeKey: tab.key, nextKey: state.nextKey + 2 };
    }
    case "split": {
      const tab = tabOf(state.tabs, action.key);
      if (!tab) return state;
      const pane = newPane(state.nextKey, action.pane);
      const next = mapTab(state, tab.key, (t) => ({
        ...t,
        panes: [...t.panes, pane],
        layout: splitLayout(t.layout, action.key, action.direction, pane.key),
        focused: pane.key,
      }));
      return { ...next, activeKey: tab.key, nextKey: state.nextKey + 1 };
    }
    case "close":
      return close(state, action.keys);
    case "activate":
      return state.activeKey === action.key ? state : { ...state, activeKey: action.key };
    case "focusPane": {
      const tab = tabOf(state.tabs, action.key);
      if (!tab) return state;
      const next = tab.focused === action.key ? state : mapTab(state, tab.key, (t) => ({ ...t, focused: action.key }));
      return reduceTabs(next, { type: "activate", key: tab.key });
    }
    case "move": {
      const tab = state.tabs.find((t) => t.key === action.key);
      if (!tab) return state;
      const rest = state.tabs.filter((t) => t.key !== action.key);
      return { ...state, tabs: [...rest.slice(0, action.index), tab, ...rest.slice(action.index)] };
    }
    case "updateTab":
      return mapTab(state, action.key, (tab) => ({ ...tab, ...action.patch }));
    case "updatePane": {
      const tab = tabOf(state.tabs, action.key);
      if (!tab) return state;
      const { patch } = action;
      return mapTab(state, tab.key, (t) => ({
        ...t,
        panes: t.panes.map((p) => (p.key === action.key ? { ...p, ...(typeof patch === "function" ? patch(p, t) : patch) } : p)),
      }));
    }
    case "togglePanel": {
      if (state.activeKey === null) return state;
      return mapTab(state, state.activeKey, (tab) => {
        if (tab.sidePanel === action.panel) return { ...tab, sidePanel: null };
        return focusedPane(tab).protocol === "ssh" ? { ...tab, sidePanel: action.panel } : tab;
      });
    }
    case "connecting": {
      const tab = tabOf(state.tabs, action.key);
      if (!tab) return state;
      return mapTab(state, tab.key, (t) => {
        const panes = t.panes.map((p) =>
          p.key === action.key
            ? { ...p, status: "connecting" as const, forwards: {}, remoteTitle: null, protocol: action.protocol }
            : p,
        );
        const sidePanel = t.focused === action.key && action.protocol !== "ssh" ? null : t.sidePanel;
        return { ...t, panes, sidePanel };
      });
    }
  }
}

export interface TabStore {
  /** The current state, also between a change and the next render. */
  get(): TabsState;
  dispatch(action: TabsAction): void;
  subscribe(listener: () => void): () => void;
}

export function createTabStore(initial: TabsState = INITIAL_TABS): TabStore {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => state,
    dispatch(action) {
      const next = reduceTabs(state, action);
      if (next === state) return;
      state = next;
      for (const listener of listeners) listener();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
}

/** The store's state, rendering again when it changes. */
export const useTabStore = (store: TabStore) => useSyncExternalStore(store.subscribe, store.get);
