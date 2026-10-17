import type { TFunction } from "i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createSessionRegistry, type PaneView } from "./sessionRegistry";
import { createTabStore } from "./tabs";

const view = (): PaneView =>
  ({
    port: {},
    screen: { size: () => ({ cols: 80, rows: 24 }), cursor: () => ({ row: 1, column: 1 }), subscribe: () => () => {} },
  }) as unknown as PaneView;

function setUp() {
  const store = createTabStore();
  store.dispatch({ type: "open", pane: { target: { kind: "local" }, protocol: "local", title: "zsh" } });
  const key = store.get().tabs[0].panes[0].key;
  const sessions = createSessionRegistry(store, {
    profiles: () => [],
    t: ((key: string) => key) as unknown as TFunction,
    announce: () => {},
    movedAway: () => {},
    stayed: () => {},
    close: () => {},
    markOutput: () => false,
    bell: () => ({ bellAlert: false, bellSound: false, bellFlash: false }),
  });
  return { key, sessions };
}

describe("createSessionRegistry", () => {
  beforeEach(() => {
    vi.stubGlobal("window", new EventTarget());
  });

  it("tells its subscribers when a pane's session comes and goes", () => {
    const { key, sessions } = setUp();
    const seen: unknown[] = [];
    const stop = sessions.subscribe(() => seen.push(sessions.get(key)));
    const first = sessions.attach(key, view());
    sessions.detach(key, first);
    // A terminal shown again for the same pane: a new session, which the subscribers hear of.
    const second = sessions.attach(key, view());
    expect(seen).toEqual([first, undefined, second]);
    expect(second).not.toBe(first);
    stop();
    sessions.detach(key, second);
    expect(seen).toHaveLength(3);
  });

  it("says nothing when a session that was replaced is detached", () => {
    const { key, sessions } = setUp();
    const first = sessions.attach(key, view());
    const second = sessions.attach(key, view());
    let calls = 0;
    sessions.subscribe(() => calls++);
    sessions.detach(key, first);
    expect(calls).toBe(0);
    expect(sessions.get(key)).toBe(second);
  });
});
