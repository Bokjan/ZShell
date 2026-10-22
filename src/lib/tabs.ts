import type { SessionId, SessionTarget } from "../ipc/api";
import { heirOf, insertLayout, mapLayout, removeFromLayout, splitLayout, type Direction, type Side } from "./layout";
import { focusedPane, type Attention, type Layout, type Pane, type SidePanel, type Tab, type TabProtocol } from "./panes";
import { createStore, type ReducerStore } from "./store";

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

/**
 * A tab moving to another window, by the keys it had there: its panes go on in the new
 * window as `Pane.arrival` says.
 */
export interface TabTransfer {
  layout: Layout;
  focused: number;
  customTitle: string | null;
  sidePanel: SidePanel | null;
  panes: Pane[];
}

/** The sessions a moving tab's panes take over: the window it goes to is offered them. */
export const transferSessions = (tab: TabTransfer): SessionId[] => tab.panes.flatMap((pane) => pane.arrival?.sessionId ?? []);

/**
 * Whether moving tab `key` (or only its pane `pane`) to a new window leaves anything in this
 * one: a window's only tab, as a whole, has nowhere new to go.
 */
export const leavesSomething = (tabs: Tab[], key: number, pane?: number) =>
  tabs.length > 1 || (pane !== undefined && (tabs.find((tab) => tab.key === key)?.panes.length ?? 0) > 1);

/** A place next to pane `key`, on its `side`, where a pane or a tab's panes go. */
export interface PaneSide {
  key: number;
  side: Side;
}

/** Where a pane moves: to a tab of its own (at `index` among the tabs, else after its tab), or next to another pane. */
export type PanePlace = { kind: "tab"; index?: number } | ({ kind: "pane" } & PaneSide);

/** A change to a pane: its fields, or a function of the pane and its tab giving them. */
export type PanePatch = Partial<Pane> | ((pane: Pane, tab: Tab) => Partial<Pane>);

export type TabsAction =
  /** Opens a tab with one pane and shows it; after the tab `after` if given, else last. */
  | { type: "open"; pane: PaneSpec; after?: number }
  /** Adds a tab from another window, with keys of this one, and shows it; at `index` if given, else last. */
  | { type: "insert"; tab: TabTransfer; index?: number }
  /** Splits pane `key`, opening `pane` right of it (`row`) or below it (`column`), and focuses it. */
  | { type: "split"; key: number; direction: Direction; pane: PaneSpec }
  /**
   * Moves pane `key` to a tab of its own, or next to another pane (of its tab, or of another,
   * which it joins), and focuses it there; a tab left without panes goes. Its terminal goes on
   * as it was (see `PaneHosts`). Not a tab's only pane to a tab of its own, nor a pane of a tab
   * that is moving to another window or to one.
   */
  | { type: "movePane"; key: number; to: PanePlace }
  /** Moves tab `key`'s panes, as they are arranged, next to a pane of another tab, which shows them; the tab goes. */
  | { type: "mergeTab"; key: number; to: PaneSide }
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
   * and the protocol and terminal type are the target's current ones. The side panel closes
   * when the focused pane is no longer SSH.
   */
  | { type: "connecting"; key: number; protocol: TabProtocol; termType: string | null }
  /** Something happened in pane `key`: its tab is marked if it is in the background. */
  | { type: "attention"; key: number; kind: Attention };

const newPane = (key: number, spec: PaneSpec): Pane => ({
  key,
  ...spec,
  remoteTitle: null,
  status: "connecting",
  termType: null,
  retryAt: null,
  sessionId: null,
  forwards: {},
  commandGroup: null,
  logPath: null,
  logStopped: false,
  transfers: 0,
  edits: 0,
  questions: 0,
  progress: null,
});

/** `state` with tab `key` made `f` of it; the same state if there is no such tab or `f` keeps it. */
function mapTab(state: TabsState, key: number, f: (tab: Tab) => Tab): TabsState {
  const tab = state.tabs.find((t) => t.key === key);
  const next = tab && f(tab);
  if (!tab || next === tab) return state;
  return { ...state, tabs: state.tabs.map((t) => (t === tab ? next! : t)) };
}

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

/** The tab shown has nothing to ask for: what marked it is in sight now. */
function seen(state: TabsState): TabsState {
  const active = activeTabOf(state);
  return active?.attention ? mapTab(state, active.key, ({ attention: _, ...tab }) => tab) : state;
}

export function reduceTabs(state: TabsState, action: TabsAction): TabsState {
  return seen(reduce(state, action));
}

/** See the `movePane` action. */
function movePane(state: TabsState, key: number, to: PanePlace): TabsState {
  const from = tabOf(state.tabs, key);
  if (!from || from.moving) return state;
  const pane = from.panes.find((p) => p.key === key)!;
  const leaf: Layout = { kind: "pane", key };
  if (to.kind === "tab") {
    if (from.panes.length === 1) return state;
    const rest = close(state, [key]);
    const tab: Tab = {
      key: state.nextKey,
      layout: leaf,
      panes: [pane],
      focused: key,
      customTitle: null,
      // The panel the pane showed goes on showing.
      sidePanel: from.focused === key && pane.protocol === "ssh" ? from.sidePanel : null,
    };
    const index = to.index ?? rest.tabs.findIndex((t) => t.key === from.key) + 1;
    const tabs = [...rest.tabs.slice(0, index), tab, ...rest.tabs.slice(index)];
    return { tabs, activeKey: tab.key, nextKey: state.nextKey + 1 };
  }
  const target = tabOf(state.tabs, to.key);
  if (!target || target.moving || to.key === key) return state;
  const rest = close(state, [key]);
  // Within its tab, it keeps its place in the list of panes, which the page's order follows.
  const panes = target === from ? from.panes : [...target.panes, pane];
  const next = mapTab(rest, target.key, (t) => ({ ...t, panes, layout: insertLayout(t.layout, to.key, to.side, leaf), focused: key }));
  return { ...next, activeKey: target.key };
}

function reduce(state: TabsState, action: TabsAction): TabsState {
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
    case "insert": {
      const keys = new Map(action.tab.panes.map((pane, i) => [pane.key, state.nextKey + i]));
      const key = state.nextKey + keys.size;
      const tab: Tab = {
        ...action.tab,
        key,
        layout: mapLayout(action.tab.layout, (old) => keys.get(old)!),
        focused: keys.get(action.tab.focused)!,
        panes: action.tab.panes.map((pane) => ({ ...pane, key: keys.get(pane.key)!, progress: null })),
      };
      const index = action.index ?? state.tabs.length;
      return { tabs: [...state.tabs.slice(0, index), tab, ...state.tabs.slice(index)], activeKey: key, nextKey: key + 1 };
    }
    case "movePane":
      return movePane(state, action.key, action.to);
    case "mergeTab": {
      const from = state.tabs.find((t) => t.key === action.key);
      const target = tabOf(state.tabs, action.to.key);
      if (!from || !target || from === target || from.moving || target.moving) return state;
      const merged: Tab = {
        ...target,
        panes: [...target.panes, ...from.panes],
        layout: insertLayout(target.layout, action.to.key, action.to.side, from.layout),
        focused: from.focused,
      };
      const tabs = state.tabs.filter((t) => t !== from).map((t) => (t === target ? merged : t));
      return { ...state, tabs, activeKey: target.key };
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
      return mapTab(state, action.key, (tab) =>
        // Nothing new: the same state, as for `updatePane`.
        Object.entries(action.patch).every(([name, value]) => Object.is(tab[name as keyof Tab], value)) ? tab : { ...tab, ...action.patch },
      );
    case "updatePane": {
      const tab = tabOf(state.tabs, action.key);
      const pane = tab?.panes.find((p) => p.key === action.key);
      if (!tab || !pane) return state;
      const patch = typeof action.patch === "function" ? action.patch(pane, tab) : action.patch;
      // Nothing new (a shell setting the same title at every prompt): the same state, which
      // renders nothing again.
      if (Object.entries(patch).every(([name, value]) => Object.is(pane[name as keyof Pane], value))) return state;
      return mapTab(state, tab.key, (t) => ({ ...t, panes: t.panes.map((p) => (p.key === action.key ? { ...p, ...patch } : p)) }));
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
            ? {
                ...p,
                status: "connecting" as const,
                forwards: {},
                remoteTitle: null,
                protocol: action.protocol,
                termType: action.termType,
                retryAt: null,
              }
            : p,
        );
        const sidePanel = t.focused === action.key && action.protocol !== "ssh" ? null : t.sidePanel;
        return { ...t, panes, sidePanel };
      });
    }
    case "attention": {
      const tab = tabOf(state.tabs, action.key);
      const outranked = tab?.attention && (tab.attention !== "output" || action.kind === "output");
      if (!tab || tab.key === state.activeKey || outranked) return state;
      return mapTab(state, tab.key, (t) => ({ ...t, attention: action.kind }));
    }
  }
}

export type TabStore = ReducerStore<TabsState, TabsAction>;

export const createTabStore = (initial: TabsState = INITIAL_TABS): TabStore => createStore(initial, reduceTabs);
