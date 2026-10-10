import { describe, expect, it } from "vitest";

import type { Folder, Profile } from "./api";
import { parseQuickConnect, rowKey, treeRows } from "./sessions";

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

describe("parseQuickConnect", () => {
  it("reads what looks like an address, SSH unless Telnet is asked for", () => {
    expect(parseQuickConnect("alice@web.example.com")).toEqual({ protocol: "ssh", username: "alice", host: "web.example.com", port: 22 });
    expect(parseQuickConnect("10.0.0.1:2222")).toEqual({ protocol: "ssh", username: "", host: "10.0.0.1", port: 2222 });
    expect(parseQuickConnect("[::1]:2222")).toEqual({ protocol: "ssh", username: "", host: "::1", port: 2222 });
    expect(parseQuickConnect("telnet router")).toEqual({ protocol: "telnet", username: "", host: "router", port: 23 });
    expect(parseQuickConnect("telnet://bob@switch:2323/")).toEqual({ protocol: "telnet", username: "bob", host: "switch", port: 2323 });
  });

  it("leaves words that are only a search alone, and refuses impossible ports", () => {
    expect(parseQuickConnect("prod")).toBeNull();
    expect(parseQuickConnect("web server")).toBeNull();
    expect(parseQuickConnect("host:70000")).toBeNull();
    expect(parseQuickConnect("host:0")).toBeNull();
  });
});
