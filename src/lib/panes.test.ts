import { describe, expect, it } from "vitest";

import { paneWork } from "./panes";
import { INITIAL_TABS, reduceTabs } from "./tabs";

const pane = reduceTabs(INITIAL_TABS, { type: "open", pane: { target: { kind: "profile", profileId: "a" }, protocol: "ssh", title: "a" } })
  .tabs[0].panes[0];

describe("paneWork", () => {
  it("is nothing for a pane whose file panel is idle", () => {
    expect(paneWork(pane)).toBeNull();
  });

  it("counts files open in an editor, connected or not", () => {
    // Saves waiting for a reconnection are lost with the pane.
    expect(paneWork({ ...pane, status: "closed", edits: 2 })).toEqual({ kind: "edits", count: 2 });
    expect(paneWork({ ...pane, status: "connected", edits: 1 })).toEqual({ kind: "edits", count: 1 });
  });

  it("names transfers first, then edits, then questions", () => {
    expect(paneWork({ ...pane, transfers: 1, edits: 1, questions: 1 })?.kind).toBe("transfers");
    expect(paneWork({ ...pane, edits: 1, questions: 1 })?.kind).toBe("edits");
    expect(paneWork({ ...pane, questions: 3 })).toEqual({ kind: "questions", count: 3 });
  });
});
