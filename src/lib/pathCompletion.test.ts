import { describe, expect, it } from "vitest";

import type { FileEntry } from "../ipc/api";
import { commonPrefix, completionTarget, folderCandidates, tabCompletion } from "./pathCompletion";

const entry = (name: string, isDir: boolean, isSymlink = false) =>
  ({ name, path: `/srv/${name}`, isDir, isSymlink, size: 0, permissions: null }) as unknown as FileEntry;

describe("path completion", () => {
  it("splits the path typed into the folder to list and the start of a name", () => {
    expect(completionTarget("/usr/lo")).toEqual({ dir: "/usr/", prefix: "lo" });
    expect(completionTarget("/usr/")).toEqual({ dir: "/usr/", prefix: "" });
    expect(completionTarget("/")).toEqual({ dir: "/", prefix: "" });
    expect(completionTarget("pro")).toEqual({ dir: "", prefix: "pro" });
  });

  it("offers folders and links to folders, not files", () => {
    const entries = [entry("logs", true), entry("lib", true, true), entry("log.txt", false), entry("Library", true)];
    expect(folderCandidates(entries, "l", false)).toEqual(["lib", "logs"]);
    expect(folderCandidates(entries, "", false)).toEqual(["lib", "Library", "logs"]);
  });

  it("ignores case when nothing matches as typed", () => {
    expect(folderCandidates([entry("Projects", true), entry("public", true)], "pro", false)).toEqual(["Projects"]);
    expect(folderCandidates([entry("Projects", true), entry("profile", true)], "pro", false)).toEqual(["profile"]);
  });

  it("offers hidden folders when they are shown or asked for", () => {
    const entries = [entry(".config", true), entry("code", true)];
    expect(folderCandidates(entries, "", false)).toEqual(["code"]);
    expect(folderCandidates(entries, "", true)).toEqual([".config", "code"]);
    expect(folderCandidates(entries, ".", false)).toEqual([".config"]);
  });

  it("completes as a shell does on Tab", () => {
    expect(commonPrefix(["library", "libexec", "lib32"])).toBe("lib");
    expect(tabCompletion("l", ["library", "libexec"])).toBe("lib");
    expect(tabCompletion("lib", ["library", "libexec"])).toBeNull();
    expect(tabCompletion("lo", ["logs"])).toBe("logs/");
    // The case of the folders found ignoring it.
    expect(tabCompletion("pro", ["Projects", "Proposals"])).toBe("Pro");
    expect(tabCompletion("x", [])).toBeNull();
  });
});
