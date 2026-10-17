import { describe, expect, it } from "vitest";

import type { Folder, Profile, TreeItem } from "../ipc/api";
import { treeRows } from "./sessions";
import { dropMove, dropOnRow, parentOf, sameDrop } from "./treeDrop";

const folder = (id: string, parent?: string): Folder => ({ id, name: id, parent });
// Only what the tree looks at.
const profile = (id: string, folder?: string) => ({ id, name: id, folder }) as Profile;
const item = (key: string): TreeItem => ({ kind: key.startsWith("f:") ? "folder" : "profile", id: key.slice(2) });

const folders = [folder("work"), folder("db", "work"), folder("home")];
const profiles = [profile("top"), profile("web", "work"), profile("pg", "db"), profile("lost", "gone")];
const tree = { folders, profiles, order: ["p:top", "f:work", "p:web", "f:db", "p:pg", "f:home", "p:lost"].map(item) };
const rows = treeRows(tree, new Set(["home"]));

describe("treeDrop", () => {
  it("finds the folder a row is in", () => {
    expect(parentOf("p:web", rows, folders)).toBe("work");
    expect(parentOf("f:db", rows, folders)).toBe("work");
    expect(parentOf("p:top", rows, folders)).toBeUndefined();
    // Its folder is gone: at the top level.
    expect(parentOf("p:lost", rows, folders)).toBeUndefined();
  });

  it("drops before or after a session, by its halves", () => {
    expect(dropOnRow(item("p:top"), "p:web", 0.4, rows, folders)).toEqual({ key: "p:web", position: "before" });
    expect(dropOnRow(item("p:top"), "p:web", 0.6, rows, folders)).toEqual({ key: "p:web", position: "after" });
    expect(dropOnRow(item("p:web"), "p:web", 0.6, rows, folders)).toBeNull();
  });

  it("drops into a folder, or around it unless it is open", () => {
    expect(dropOnRow(item("p:top"), "f:home", 0.1, rows, folders)).toEqual({ key: "f:home", position: "before" });
    expect(dropOnRow(item("p:top"), "f:home", 0.5, rows, folders)).toEqual({ key: "f:home", position: "into" });
    expect(dropOnRow(item("p:top"), "f:home", 0.9, rows, folders)).toEqual({ key: "f:home", position: "after" });
    // Open: its rows follow it.
    expect(dropOnRow(item("p:top"), "f:work", 0.9, rows, folders)).toEqual({ key: "f:work", position: "into" });
  });

  it("keeps a folder out of itself and its subfolders", () => {
    expect(dropOnRow(item("f:work"), "f:db", 0.5, rows, folders)).toBeNull();
    expect(dropOnRow(item("f:work"), "p:pg", 0.2, rows, folders)).toBeNull();
    expect(dropOnRow(item("f:db"), "f:home", 0.5, rows, folders)).toEqual({ key: "f:home", position: "into" });
  });

  it("moves into the folder, before the row that follows the drop there", () => {
    expect(dropMove(item("p:top"), { key: null }, tree, rows)).toEqual({ parent: null, before: null });
    expect(dropMove(item("p:top"), { key: "f:home", position: "into" }, tree, rows)).toEqual({ parent: "home", before: null });
    expect(dropMove(item("p:top"), { key: "p:web", position: "after" }, tree, rows)).toEqual({ parent: "work", before: item("f:db") });
    expect(dropMove(item("p:top"), { key: "f:db", position: "after" }, tree, rows)).toEqual({ parent: "work", before: null });
    // Moving down among its own peers: the row itself doesn't count.
    expect(dropMove(item("p:top"), { key: "f:work", position: "after" }, tree, rows)).toEqual({ parent: null, before: item("f:home") });
  });

  it("tells drops apart", () => {
    expect(sameDrop({ key: "p:a", position: "before" }, { key: "p:a", position: "before" })).toBe(true);
    expect(sameDrop({ key: "p:a", position: "before" }, { key: "p:a", position: "after" })).toBe(false);
    expect(sameDrop({ key: null }, { key: null })).toBe(true);
    expect(sameDrop(null, { key: null })).toBe(false);
  });
});
