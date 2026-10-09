import { describe, expect, it } from "vitest";

import type { PaneSpec, TabsAction, TabsState } from "./tabs";
import { activeTabOf, createTabStore, INITIAL_TABS, reduceTabs } from "./tabs";

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
    expect(tab.panes[0]).toMatchObject({ status: "connecting", sessionId: null, reconnectKey: 0, transfers: 0 });
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
    state = reduceTabs(state, { type: "updatePane", key, patch: (pane) => ({ reconnectKey: pane.reconnectKey + 1 }) });
    state = reduceTabs(state, { type: "updatePane", key, patch: (pane) => ({ reconnectKey: pane.reconnectKey + 1 }) });
    expect(state.tabs[0].panes[0].reconnectKey).toBe(2);
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
    state = reduceTabs(state, { type: "updatePane", key, patch: { status: "connected", remoteTitle: "vim", forwards: { r: { type: "starting" } } } });
    state = reduceTabs(state, { type: "connecting", key, protocol: "ssh" });
    expect(state.tabs[0].sidePanel).toBe("files");
    expect(state.tabs[0].panes[0]).toMatchObject({ status: "connecting", remoteTitle: null, forwards: {} });
    state = reduceTabs(state, { type: "connecting", key, protocol: "telnet" });
    expect(state.tabs[0].sidePanel).toBeNull();
    expect(state.tabs[0].panes[0].protocol).toBe("telnet");
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
