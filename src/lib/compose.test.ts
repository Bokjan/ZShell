import { describe, expect, it } from "vitest";

import type { Compose } from "./compose";
import { syncTargets } from "./compose";
import type { Pane } from "./panes";
import type { PaneSpec, TabsAction } from "./tabs";
import { INITIAL_TABS, reduceTabs } from "./tabs";

const ssh = (profileId: string): PaneSpec => ({ target: { kind: "profile", profileId }, protocol: "ssh", title: profileId });
const run = (...actions: TabsAction[]) => actions.reduce(reduceTabs, INITIAL_TABS);
const syncing: Compose = { open: true, scope: "tab", selected: [], sync: true };

/** A tab with three panes, the last one focused; `connected` says which are connected. */
function threePanes(connected: [boolean, boolean, boolean]) {
  let state = run({ type: "open", pane: ssh("a") });
  const first = state.tabs[0].panes[0].key;
  state = reduceTabs(state, { type: "split", key: first, direction: "row", pane: ssh("b") });
  state = reduceTabs(state, { type: "split", key: state.tabs[0].focused, direction: "row", pane: ssh("c") });
  const tabs = state.tabs.map((tab) => ({
    ...tab,
    panes: tab.panes.map(
      (pane, i): Pane => (connected[i] ? { ...pane, status: "connected", sessionId: 100 + i } : { ...pane, status: "connecting" }),
    ),
  }));
  return { tabs, activeKey: state.activeKey, keys: tabs[0].panes.map((pane) => pane.key), focused: tabs[0].focused };
}

describe("syncTargets", () => {
  it("sends what is typed in the focused pane to the other connected panes", () => {
    const { tabs, activeKey, keys, focused } = threePanes([true, true, true]);
    expect(focused).toBe(keys[2]);
    expect(syncTargets(syncing, tabs, activeKey, focused).map((pane) => pane.key)).toEqual([keys[0], keys[1]]);
  });

  it("sends nothing while the focused pane is still connecting (its own prompts)", () => {
    const { tabs, activeKey, focused } = threePanes([true, true, false]);
    expect(syncTargets(syncing, tabs, activeKey, focused)).toEqual([]);
  });

  it("skips panes that aren't connected", () => {
    const { tabs, activeKey, keys, focused } = threePanes([false, true, true]);
    expect(syncTargets(syncing, tabs, activeKey, focused).map((pane) => pane.key)).toEqual([keys[1]]);
  });

  it("sends nothing without sync, or from a pane that isn't focused", () => {
    const { tabs, activeKey, keys, focused } = threePanes([true, true, true]);
    expect(syncTargets({ ...syncing, sync: false }, tabs, activeKey, focused)).toEqual([]);
    expect(syncTargets({ ...syncing, open: false }, tabs, activeKey, focused)).toEqual([]);
    expect(syncTargets(syncing, tabs, activeKey, keys[0])).toEqual([]);
  });
});
