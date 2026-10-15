import { describe, expect, it } from "vitest";

import type { Folder, Profile, Proxy } from "../ipc/api";
import { createDirectoryStore, INITIAL_DIRECTORY, paneProfile, recentProfiles, reduceDirectory, type DirectoryAction } from "./directory";
import type { Pane } from "./panes";

// Only what the directory looks at.
const profile = (id: string, name = id) => ({ id, name }) as Profile;
const folder: Folder = { id: "work", name: "Work" };
const proxy = { id: "socks", name: "socks" } as Proxy;
const loaded: DirectoryAction = { type: "loaded", profiles: [profile("a"), profile("b")], folders: [folder], order: [], proxies: [proxy] };
const run = (...actions: DirectoryAction[]) => actions.reduce(reduceDirectory, INITIAL_DIRECTORY);

describe("reduceDirectory", () => {
  it("is loaded once read, with what was read", () => {
    expect(INITIAL_DIRECTORY.loaded).toBe(false);
    expect(run(loaded)).toMatchObject({ loaded: true, profiles: [profile("a"), profile("b")], folders: [folder], proxies: [proxy] });
  });

  it("replaces a saved session in place, and ignores one it doesn't have", () => {
    const state = run(loaded);
    expect(reduceDirectory(state, { type: "profileSaved", profile: profile("b", "renamed") }).profiles).toEqual([
      profile("a"),
      profile("b", "renamed"),
    ]);
    expect(reduceDirectory(state, { type: "profileSaved", profile: profile("gone") })).toBe(state);
  });

  it("changes only when the recent sessions do", () => {
    const state = run({ type: "recent", ids: ["b", "a"] });
    expect(state.recent).toEqual(["b", "a"]);
    expect(reduceDirectory(state, { type: "recent", ids: ["b", "a"] })).toBe(state);
  });

  it("forgets everything on reset", () => {
    expect(run(loaded, { type: "recent", ids: ["a"] }, { type: "reset" })).toBe(INITIAL_DIRECTORY);
  });
});

describe("recentProfiles", () => {
  it("lists the recent sessions that still exist, most recent first, up to the limit", () => {
    const profiles = ["a", "b", "c", "d", "e", "f"].map((id) => profile(id));
    const state = run(
      { type: "loaded", profiles, folders: [], order: [], proxies: [] },
      { type: "recent", ids: ["f", "gone", "a", "e", "b", "d", "c"] },
    );
    expect(recentProfiles(state).map((p) => p.id)).toEqual(["f", "a", "e", "b", "d"]);
  });
});

describe("paneProfile", () => {
  it("finds a saved session's pane's session, and nothing for other panes", () => {
    const profiles = [profile("a")];
    expect(paneProfile(profiles, { target: { kind: "profile", profileId: "a" } } as Pane)).toBe(profiles[0]);
    expect(paneProfile(profiles, { target: { kind: "profile", profileId: "gone" } } as Pane)).toBeUndefined();
    expect(paneProfile(profiles, { target: { kind: "local" } } as Pane)).toBeUndefined();
  });
});

describe("createDirectoryStore", () => {
  it("tells its listeners of changes, and only of changes", () => {
    const store = createDirectoryStore();
    let heard = 0;
    const unsubscribe = store.subscribe(() => heard++);
    store.dispatch(loaded);
    store.dispatch({ type: "profileSaved", profile: profile("gone") });
    expect(heard).toBe(1);
    unsubscribe();
    store.dispatch({ type: "reset" });
    expect([heard, store.get()]).toEqual([1, INITIAL_DIRECTORY]);
  });
});
