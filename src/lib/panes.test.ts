import { describe, expect, it } from "vitest";

import type { Profile } from "../ipc/api";
import { closesOnExit, keepsAwake, paneWork, targetProfile } from "./panes";
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

describe("closesOnExit", () => {
  const local = { ...pane, target: { kind: "local" as const } };

  it("closes a pane whose shell the user exited, whatever the status", () => {
    expect(closesOnExit(pane, 0, null, true)).toBe(true);
    expect(closesOnExit(pane, 1, null, true)).toBe(true);
    // Telnet servers, and SSH servers that don't send one, report no status.
    expect(closesOnExit(pane, null, null, true)).toBe(true);
    expect(closesOnExit(local, 0, null, true)).toBe(true);
    expect(closesOnExit(local, 130, null, true)).toBe(true);
  });

  it("keeps it when the user never typed into the shell", () => {
    expect(closesOnExit(pane, 0, null, false)).toBe(false);
    expect(closesOnExit(local, 0, null, false)).toBe(false);
  });

  it("keeps a shell ended by a signal", () => {
    expect(closesOnExit(local, null, "Killed: 9", true)).toBe(false);
    expect(closesOnExit(pane, null, "SIGKILL", true)).toBe(false);
    // A local shell reports a signal without a status.
    expect(closesOnExit(local, null, null, true)).toBe(false);
  });

  it("keeps it while closing would end work of the file panel", () => {
    expect(closesOnExit({ ...pane, transfers: 1 }, 0, null, true)).toBe(false);
    expect(closesOnExit({ ...pane, edits: 1 }, 0, null, true)).toBe(false);
  });
});

describe("keepsAwake", () => {
  it("keeps the system awake while a remote pane is connected, connecting or waiting to reconnect", () => {
    expect(keepsAwake({ ...pane, status: "connected" })).toBe(true);
    expect(keepsAwake({ ...pane, status: "connecting" })).toBe(true);
    expect(keepsAwake({ ...pane, status: "closed", retryAt: Date.now() + 2000 })).toBe(true);
    expect(keepsAwake({ ...pane, status: "closed", retryAt: null })).toBe(false);
  });

  it("never for a local terminal", () => {
    expect(keepsAwake({ ...pane, target: { kind: "local" }, protocol: "local", status: "connected" })).toBe(false);
  });
});

describe("targetProfile", () => {
  it("finds the saved session of a target, and nothing for others", () => {
    const profiles = [{ id: "a" } as Profile];
    expect(targetProfile({ kind: "profile", profileId: "a" }, profiles)).toBe(profiles[0]);
    expect(targetProfile({ kind: "profile", profileId: "gone" }, profiles)).toBeUndefined();
    expect(targetProfile({ kind: "local" }, profiles)).toBeUndefined();
  });
});
