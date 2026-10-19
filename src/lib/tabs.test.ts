import { describe, expect, it } from "vitest";

import type { PaneSpec, TabsAction, TabsState } from "./tabs";
import { activeTabOf, createTabStore, INITIAL_TABS, leavesSomething, reduceTabs, transferSessions } from "./tabs";

const local: PaneSpec = { target: { kind: "local" }, protocol: "local", title: "zsh" };
const ssh = (profileId: string): PaneSpec => ({ target: { kind: "profile", profileId }, protocol: "ssh", title: profileId });

const run = (...actions: TabsAction[]) => actions.reduce(reduceTabs, INITIAL_TABS);
const paneKeys = (state: TabsState) => state.tabs.map((tab) => tab.panes.map((pane) => pane.key));

describe("open", () => {
  it("appends a tab with one pane and shows it", () => {
    const state = run({ type: "open", pane: local }, { type: "open", pane: ssh("a") });
    expect(state.tabs.map((tab) => tab.panes[0].title)).toEqual(["zsh", "a"]);
    expect(state.activeKey).toBe(state.tabs[1].key);
    const tab = state.tabs[1];
    expect(tab.focused).toBe(tab.panes[0].key);
    expect(tab.layout).toEqual({ kind: "pane", key: tab.panes[0].key });
    expect(tab.panes[0]).toMatchObject({ status: "connecting", sessionId: null, transfers: 0 });
  });

  it("never gives a tab and a pane the same key", () => {
    const state = run({ type: "open", pane: local }, { type: "open", pane: local });
    const keys = state.tabs.flatMap((tab) => [tab.key, ...tab.panes.map((pane) => pane.key)]);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("puts a copy right after its tab", () => {
    let state = run({ type: "open", pane: ssh("a") }, { type: "open", pane: ssh("b") });
    state = reduceTabs(state, { type: "open", pane: ssh("a"), after: state.tabs[0].key });
    expect(state.tabs.map((tab) => tab.panes[0].title)).toEqual(["a", "a", "b"]);
    expect(state.activeKey).toBe(state.tabs[1].key);
  });
});

describe("insert", () => {
  it("adds a tab from another window with keys of this one, and shows it", () => {
    // A split tab, as another window has it.
    const there = run({ type: "open", pane: local }, { type: "open", pane: ssh("a") });
    const moving = reduceTabs(there, { type: "split", key: there.tabs[1].panes[0].key, direction: "column", pane: ssh("b") }).tabs[1];
    const here = run({ type: "open", pane: local });
    const state = reduceTabs(here, { type: "insert", tab: moving });
    const tab = state.tabs[1];
    expect(state.activeKey).toBe(tab.key);
    expect(tab.panes.map((pane) => pane.title)).toEqual(["a", "b"]);
    const keys = state.tabs.flatMap((t) => [t.key, ...t.panes.map((pane) => pane.key)]);
    expect(new Set(keys).size).toBe(keys.length);
    expect(tab.focused).toBe(tab.panes[1].key);
    expect(tab.layout).toMatchObject({
      kind: "split",
      direction: "column",
      children: [
        { kind: "pane", key: tab.panes[0].key },
        { kind: "pane", key: tab.panes[1].key },
      ],
    });
    // Keys given later don't collide with the inserted ones.
    const next = reduceTabs(state, { type: "open", pane: local });
    expect(keys).not.toContain(next.tabs[2].key);
    expect(keys).not.toContain(next.tabs[2].panes[0].key);
  });

  it("lists the sessions a moving tab takes over", () => {
    const there = run({ type: "open", pane: ssh("a") });
    const [a] = there.tabs[0].panes;
    const from = { window: "main", pane: a.key };
    const arriving = (sessionId: number | null) => ({
      ...a,
      arrival: { sessionId, connected: true, snapshot: null, reconnect: false, from },
    });
    const tab = { ...there.tabs[0], panes: [arriving(7), arriving(null), a, arriving(9)] };
    expect(transferSessions(tab)).toEqual([7, 9]);
  });

  it("puts the tab where it was dropped", () => {
    const moving = run({ type: "open", pane: ssh("moved") }).tabs[0];
    const here = run({ type: "open", pane: ssh("a") }, { type: "open", pane: ssh("b") });
    const state = reduceTabs(here, { type: "insert", tab: moving, index: 1 });
    expect(state.tabs.map((tab) => tab.panes[0].title)).toEqual(["a", "moved", "b"]);
  });
});

describe("split", () => {
  it("adds the pane beside the split one, focuses it and shows its tab", () => {
    let state = run({ type: "open", pane: ssh("a") }, { type: "open", pane: local });
    const first = state.tabs[0];
    state = reduceTabs(state, { type: "split", key: first.panes[0].key, direction: "row", pane: ssh("b") });
    const tab = state.tabs[0];
    const added = tab.panes[1];
    expect(added.title).toBe("b");
    expect(tab.focused).toBe(added.key);
    expect(state.activeKey).toBe(tab.key);
    expect(tab.layout).toMatchObject({ kind: "split", direction: "row", sizes: [0.5, 0.5] });
  });

  it("ignores a pane that no longer exists", () => {
    const state = run({ type: "open", pane: local });
    expect(reduceTabs(state, { type: "split", key: 999, direction: "row", pane: local })).toBe(state);
  });
});

describe("movePane", () => {
  /** Tab "a" split into a | b | c, and tab "d". */
  const setup = () => {
    let state = run({ type: "open", pane: ssh("a") }, { type: "open", pane: ssh("d") });
    const a = state.tabs[0].panes[0].key;
    state = reduceTabs(state, { type: "split", key: a, direction: "row", pane: ssh("b") });
    state = reduceTabs(state, { type: "split", key: state.tabs[0].panes[1].key, direction: "row", pane: ssh("c") });
    const [b, c] = state.tabs[0].panes.slice(1).map((pane) => pane.key);
    return { state, a, b, c, d: state.tabs[1].panes[0].key };
  };
  const titles = (state: TabsState) => state.tabs.map((tab) => tab.panes.map((pane) => pane.title).join(""));

  it("moves a pane to a tab of its own after its tab, the same pane, and shows it", () => {
    const { state: before, a, b, c } = setup();
    const pane = before.tabs[0].panes[1];
    const state = reduceTabs(before, { type: "movePane", key: b, to: { kind: "tab" } });
    expect(titles(state)).toEqual(["ac", "b", "d"]);
    expect(state.tabs[1].panes[0]).toBe(pane);
    expect(state.tabs[1]).toMatchObject({ focused: b, layout: { kind: "pane", key: b }, customTitle: null });
    expect(state.activeKey).toBe(state.tabs[1].key);
    // Its tab keeps the focus where it was; one the pane had goes to its neighbor.
    expect(state.tabs[0].focused).toBe(c);
    const focusedAway = reduceTabs(reduceTabs(before, { type: "focusPane", key: b }), { type: "movePane", key: b, to: { kind: "tab" } });
    expect(focusedAway.tabs[0].focused).toBe(a);
    expect(paneKeys(state).flat().length).toBe(4);
    // Where it was asked to go, and keys stay unique.
    const last = reduceTabs(before, { type: "movePane", key: b, to: { kind: "tab", index: 2 } });
    expect(titles(last)).toEqual(["ac", "d", "b"]);
    const keys = last.tabs.flatMap((tab) => [tab.key, ...tab.panes.map((p) => p.key)]);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("takes the side panel along when the pane showed it", () => {
    let { state, c } = setup();
    state = reduceTabs(state, { type: "togglePanel", panel: "files" });
    state = reduceTabs(state, { type: "movePane", key: c, to: { kind: "tab" } });
    expect(state.tabs.map((tab) => tab.sidePanel)).toEqual(["files", "files", null]);
  });

  it("leaves a tab's only pane where it is", () => {
    const { state, d } = setup();
    expect(reduceTabs(state, { type: "movePane", key: d, to: { kind: "tab" } })).toBe(state);
  });

  it("moves a pane next to another of its tab, keeping the order of the panes", () => {
    const { state: before, a, c } = setup();
    const state = reduceTabs(before, { type: "movePane", key: a, to: { kind: "pane", key: c, side: "down" } });
    expect(titles(state)).toEqual(["abc", "d"]);
    expect(state.tabs[0].layout).toMatchObject({
      kind: "split",
      direction: "row",
      children: [
        { kind: "pane", key: state.tabs[0].panes[1].key },
        { kind: "split", direction: "column", children: [{ key: c }, { key: a }] },
      ],
    });
    expect(state.tabs[0].focused).toBe(a);
    expect(reduceTabs(before, { type: "movePane", key: a, to: { kind: "pane", key: a, side: "down" } })).toBe(before);
  });

  it("moves a pane into another tab, which shows it; a tab left empty goes", () => {
    const { state: before, b, d } = setup();
    let state = reduceTabs(before, { type: "movePane", key: b, to: { kind: "pane", key: d, side: "left" } });
    expect(titles(state)).toEqual(["ac", "db"]);
    expect(state.tabs[1].layout).toMatchObject({ kind: "split", children: [{ key: b }, { key: d }] });
    expect(state.tabs[1].focused).toBe(b);
    expect(state.activeKey).toBe(state.tabs[1].key);
    state = reduceTabs(state, { type: "movePane", key: d, to: { kind: "pane", key: state.tabs[0].panes[0].key, side: "right" } });
    expect(titles(state)).toEqual(["acd", "b"]);
    state = reduceTabs(state, { type: "movePane", key: b, to: { kind: "pane", key: d, side: "up" } });
    expect(titles(state)).toEqual(["acdb"]);
    expect(state.activeKey).toBe(state.tabs[0].key);
  });

  it("leaves the panes of a tab moving to another window", () => {
    const { state: before, b, d } = setup();
    const moving = reduceTabs(before, { type: "updateTab", key: before.tabs[1].key, patch: { moving: true } });
    expect(reduceTabs(moving, { type: "movePane", key: b, to: { kind: "pane", key: d, side: "left" } })).toBe(moving);
    expect(reduceTabs(moving, { type: "movePane", key: d, to: { kind: "pane", key: b, side: "left" } })).toBe(moving);
  });
});

describe("leavesSomething", () => {
  it("leaves a window's only tab whole, but lets one of its panes go", () => {
    let state = run({ type: "open", pane: ssh("a") });
    const tab = state.tabs[0];
    const a = tab.panes[0].key;
    expect(leavesSomething(state.tabs, tab.key)).toBe(false);
    expect(leavesSomething(state.tabs, tab.key, a)).toBe(false);
    state = reduceTabs(state, { type: "split", key: a, direction: "row", pane: ssh("b") });
    expect(leavesSomething(state.tabs, tab.key)).toBe(false);
    expect(leavesSomething(state.tabs, tab.key, a)).toBe(true);
    state = reduceTabs(state, { type: "open", pane: ssh("c") });
    expect(leavesSomething(state.tabs, tab.key)).toBe(true);
  });
});

describe("mergeTab", () => {
  it("brings a tab's panes in next to a pane of another, as they were, and the tab goes", () => {
    let state = run({ type: "open", pane: ssh("a") }, { type: "open", pane: ssh("b") }, { type: "open", pane: ssh("c") });
    const [a, b, c] = state.tabs.map((tab) => tab.panes[0].key);
    state = reduceTabs(state, { type: "split", key: b, direction: "column", pane: ssh("e") });
    state = reduceTabs(state, { type: "focusPane", key: b });
    const merged = reduceTabs(state, { type: "mergeTab", key: state.tabs[1].key, to: { key: a, side: "right" } });
    expect(merged.tabs.map((tab) => tab.panes.map((pane) => pane.title).join(""))).toEqual(["abe", "c"]);
    expect(merged.tabs[0].focused).toBe(b);
    expect(merged.activeKey).toBe(merged.tabs[0].key);
    expect(merged.tabs[0].layout).toMatchObject({
      kind: "split",
      direction: "row",
      children: [{ key: a }, { kind: "split", direction: "column" }],
    });
    // Not into itself.
    expect(reduceTabs(state, { type: "mergeTab", key: state.tabs[2].key, to: { key: c, side: "left" } })).toBe(state);
  });
});

describe("close", () => {
  it("activates the next tab to the right, else the one to the left", () => {
    let state = run({ type: "open", pane: local }, { type: "open", pane: local }, { type: "open", pane: local });
    const [a, b, c] = state.tabs;
    state = reduceTabs(state, { type: "activate", key: b.key });
    state = reduceTabs(state, { type: "close", keys: [b.panes[0].key] });
    expect(state.activeKey).toBe(c.key);
    state = reduceTabs(state, { type: "close", keys: [c.panes[0].key] });
    expect(state.activeKey).toBe(a.key);
    state = reduceTabs(state, { type: "close", keys: [a.panes[0].key] });
    expect(state).toMatchObject({ tabs: [], activeKey: null });
  });

  it("keeps the active tab when another closes", () => {
    let state = run({ type: "open", pane: local }, { type: "open", pane: local });
    const [a, b] = state.tabs;
    state = reduceTabs(state, { type: "close", keys: [a.panes[0].key] });
    expect(state.activeKey).toBe(b.key);
  });

  it("gives a closed pane's focus to its neighbor and keeps the tab", () => {
    let state = run({ type: "open", pane: local });
    const first = state.tabs[0].panes[0].key;
    state = reduceTabs(state, { type: "split", key: first, direction: "column", pane: local });
    const second = state.tabs[0].focused;
    state = reduceTabs(state, { type: "close", keys: [second] });
    expect(paneKeys(state)).toEqual([[first]]);
    expect(state.tabs[0].focused).toBe(first);
    expect(state.tabs[0].layout).toEqual({ kind: "pane", key: first });
  });

  it("closes panes of several tabs at once, like two that exit together", () => {
    let state = run({ type: "open", pane: local }, { type: "open", pane: local }, { type: "open", pane: local });
    const [a, b, c] = state.tabs;
    state = reduceTabs(state, { type: "close", keys: [a.panes[0].key, c.panes[0].key] });
    expect(state.tabs.map((tab) => tab.key)).toEqual([b.key]);
    expect(state.activeKey).toBe(b.key);
  });
});

describe("focusPane", () => {
  it("activates the pane's tab and focuses the pane", () => {
    let state = run({ type: "open", pane: local });
    const first = state.tabs[0].panes[0].key;
    state = reduceTabs(state, { type: "split", key: first, direction: "row", pane: local });
    state = reduceTabs(state, { type: "open", pane: local });
    state = reduceTabs(state, { type: "focusPane", key: first });
    expect(state.activeKey).toBe(state.tabs[0].key);
    expect(state.tabs[0].focused).toBe(first);
  });
});

describe("move", () => {
  it("moves a tab to an index among the others", () => {
    let state = run({ type: "open", pane: ssh("a") }, { type: "open", pane: ssh("b") }, { type: "open", pane: ssh("c") });
    state = reduceTabs(state, { type: "move", key: state.tabs[0].key, index: 2 });
    expect(state.tabs.map((tab) => tab.panes[0].title)).toEqual(["b", "c", "a"]);
  });
});

describe("updatePane", () => {
  it("applies a function of the pane and its tab", () => {
    let state = run({ type: "open", pane: ssh("a") });
    const key = state.tabs[0].panes[0].key;
    state = reduceTabs(state, { type: "updatePane", key, patch: (pane) => ({ transfers: pane.transfers + 1 }) });
    state = reduceTabs(state, { type: "updatePane", key, patch: (pane) => ({ transfers: pane.transfers + 1 }) });
    expect(state.tabs[0].panes[0].transfers).toBe(2);
  });

  it("keeps the same state when nothing changes", () => {
    const state = run({ type: "open", pane: ssh("a") });
    const key = state.tabs[0].panes[0].key;
    const titled = reduceTabs(state, { type: "updatePane", key, patch: { remoteTitle: "vim", transfers: 0 } });
    expect(titled).not.toBe(state);
    expect(reduceTabs(titled, { type: "updatePane", key, patch: { remoteTitle: "vim" } })).toBe(titled);
    expect(reduceTabs(titled, { type: "updatePane", key, patch: () => ({ transfers: 0 }) })).toBe(titled);
  });
});

describe("side panels", () => {
  it("opens only for an SSH pane, and closes whatever the pane", () => {
    let state = run({ type: "open", pane: local });
    state = reduceTabs(state, { type: "togglePanel", panel: "files" });
    expect(activeTabOf(state)!.sidePanel).toBeNull();
    state = run({ type: "open", pane: ssh("a") }, { type: "togglePanel", panel: "files" });
    expect(activeTabOf(state)!.sidePanel).toBe("files");
    state = reduceTabs(state, { type: "togglePanel", panel: "forwards" });
    expect(activeTabOf(state)!.sidePanel).toBe("forwards");
    state = reduceTabs(state, { type: "togglePanel", panel: "forwards" });
    expect(activeTabOf(state)!.sidePanel).toBeNull();
  });

  it("closes when the focused pane's session reconnects as another protocol", () => {
    let state = run({ type: "open", pane: ssh("a") }, { type: "togglePanel", panel: "files" });
    const key = state.tabs[0].panes[0].key;
    const patch = { status: "connected" as const, remoteTitle: "vim", forwards: { r: { type: "starting" as const } }, retryAt: 5 };
    state = reduceTabs(state, { type: "updatePane", key, patch });
    state = reduceTabs(state, { type: "connecting", key, protocol: "ssh", termType: "xterm-256color" });
    expect(state.tabs[0].sidePanel).toBe("files");
    expect(state.tabs[0].panes[0]).toMatchObject({
      status: "connecting",
      remoteTitle: null,
      forwards: {},
      termType: "xterm-256color",
      retryAt: null,
    });
    state = reduceTabs(state, { type: "connecting", key, protocol: "telnet", termType: "vt100" });
    expect(state.tabs[0].sidePanel).toBeNull();
    expect(state.tabs[0].panes[0]).toMatchObject({ protocol: "telnet", termType: "vt100" });
  });
});

describe("store", () => {
  it("has the new state at once and tells subscribers only of changes", () => {
    const store = createTabStore();
    let calls = 0;
    store.subscribe(() => calls++);
    store.dispatch({ type: "open", pane: local });
    expect(store.get().tabs).toHaveLength(1);
    store.dispatch({ type: "activate", key: store.get().activeKey });
    expect(calls).toBe(1);
  });
});

describe("attention", () => {
  // Two tabs, the second shown.
  const twoTabs = () => run({ type: "open", pane: ssh("a") }, { type: "open", pane: ssh("b") });
  const paneOf = (state: TabsState, index: number) => state.tabs[index].panes[0].key;

  it("marks a tab in the background, not the one shown", () => {
    let state = twoTabs();
    state = reduceTabs(state, { type: "attention", key: paneOf(state, 1), kind: "output" });
    expect(state.tabs[1].attention).toBeUndefined();
    state = reduceTabs(state, { type: "attention", key: paneOf(state, 0), kind: "output" });
    expect(state.tabs[0].attention).toBe("output");
  });

  it("lets a bell outrank output, not the other way round", () => {
    let state = twoTabs();
    const key = paneOf(state, 0);
    state = reduceTabs(state, { type: "attention", key, kind: "output" });
    state = reduceTabs(state, { type: "attention", key, kind: "bell" });
    expect(state.tabs[0].attention).toBe("bell");
    const before = state;
    state = reduceTabs(state, { type: "attention", key, kind: "output" });
    expect(state).toBe(before);
    // Of a bell and a notification, the first stays.
    state = reduceTabs(state, { type: "attention", key, kind: "notice" });
    expect(state).toBe(before);
    state = reduceTabs(twoTabs(), { type: "attention", key, kind: "notice" });
    expect(state.tabs[0].attention).toBe("notice");
  });

  it("clears the mark once the tab is shown, however it comes to be", () => {
    let state = twoTabs();
    state = reduceTabs(state, { type: "attention", key: paneOf(state, 0), kind: "bell" });
    expect(reduceTabs(state, { type: "activate", key: state.tabs[0].key }).tabs[0].attention).toBeUndefined();
    expect(reduceTabs(state, { type: "focusPane", key: paneOf(state, 0) }).tabs[0].attention).toBeUndefined();
    // Closing the tab shown shows the one left.
    expect(reduceTabs(state, { type: "close", keys: [paneOf(state, 1)] }).tabs[0].attention).toBeUndefined();
  });
});
