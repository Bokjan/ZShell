import { describe, expect, it } from "vitest";

import type { Folder, Profile } from "./api";
import { rowKey, treeRows } from "./sessions";

const folder = (id: string, parent?: string): Folder => ({ id, name: id, parent });
// Only what the tree looks at.
const profile = (id: string, folder?: string) => ({ id, name: id, folder }) as Profile;

describe("treeRows", () => {
  it("shows folders before sessions, nested, and hides what a collapsed folder holds", () => {
    const folders = [folder("work"), folder("db", "work")];
    const profiles = [profile("top"), profile("web", "work"), profile("pg", "db")];
    expect(treeRows(folders, profiles, new Set()).map(rowKey)).toEqual(["f:work", "f:db", "p:pg", "p:web", "p:top"]);
    expect(treeRows(folders, profiles, new Set(["work"])).map(rowKey)).toEqual(["f:work", "p:top"]);
  });

  it("shows a session whose folder is missing at the top level", () => {
    const rows = treeRows([folder("work")], [profile("lost", "gone"), profile("web", "work")], new Set());
    expect(rows.map((row) => [rowKey(row), row.depth])).toEqual([
      ["f:work", 0],
      ["p:web", 1],
      ["p:lost", 0],
    ]);
  });
});
