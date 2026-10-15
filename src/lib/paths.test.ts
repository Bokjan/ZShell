import { afterEach, describe, expect, it, vi } from "vitest";

/** `./paths` as on Windows, or elsewhere. */
async function paths(windows: boolean) {
  vi.resetModules();
  vi.doMock("./platform", () => ({ isWindows: windows }));
  return import("./paths");
}

afterEach(() => {
  vi.doUnmock("./platform");
});

describe("startsWithHome", () => {
  it("knows `~` alone or followed by a separator", async () => {
    const { startsWithHome } = await paths(false);
    for (const path of ["~", "~/.ssh/id_ed25519", "~\\.ssh\\id_ed25519"]) expect(startsWithHome(path), path).toBe(true);
    for (const path of ["~alice/.ssh", "/home/alice/~", "", "./~"]) expect(startsWithHome(path), path).toBe(false);
  });
});

describe("expandHome", () => {
  it("puts the home folder in place of `~`, with the platform's separators", async () => {
    const unix = await paths(false);
    expect(unix.expandHome("~/.ssh/id_ed25519", "/home/alice")).toBe("/home/alice/.ssh/id_ed25519");
    expect(unix.expandHome("~\\.ssh\\\\id_rsa", "/home/alice/")).toBe("/home/alice/.ssh/id_rsa");
    expect(unix.expandHome("~", "/home/alice")).toBe("/home/alice");
    expect(unix.expandHome("~/", "/home/alice")).toBe("/home/alice");

    const windows = await paths(true);
    expect(windows.expandHome("~/.ssh/id_ed25519", "C:\\Users\\alice")).toBe("C:\\Users\\alice\\.ssh\\id_ed25519");
    expect(windows.expandHome("~\\keys//work", "C:\\Users\\alice\\")).toBe("C:\\Users\\alice\\keys\\work");
  });

  it("leaves other paths as they are", async () => {
    const { expandHome } = await paths(false);
    for (const path of ["/etc/ssh/key", "~alice/key", "relative/key", ""]) expect(expandHome(path, "/home/alice")).toBe(path);
  });
});

describe("contractHome", () => {
  it("writes a path in the home folder as `~/…` with forward slashes", async () => {
    const unix = await paths(false);
    expect(unix.contractHome("/home/alice/.ssh/id_ed25519", "/home/alice")).toBe("~/.ssh/id_ed25519");
    expect(unix.contractHome("/home/alice/.ssh/id_ed25519", "/home/alice/")).toBe("~/.ssh/id_ed25519");

    // Windows doesn't tell case apart in paths.
    const windows = await paths(true);
    expect(windows.contractHome("c:\\users\\Alice\\.ssh\\id_ed25519", "C:\\Users\\alice")).toBe("~/.ssh/id_ed25519");
  });

  it("leaves paths outside the home folder as they are", async () => {
    const unix = await paths(false);
    // Only inside it: not a folder whose name starts with the home folder's.
    for (const path of ["/home/alice", "/home/alicebob/key", "/etc/ssh/key", "/Home/alice/key"]) {
      expect(unix.contractHome(path, "/home/alice"), path).toBe(path);
    }
    const windows = await paths(true);
    expect(windows.contractHome("D:\\keys\\work", "C:\\Users\\alice")).toBe("D:\\keys\\work");
  });

  it("undoes expandHome", async () => {
    const { contractHome, expandHome } = await paths(false);
    expect(contractHome(expandHome("~/a/b", "/home/alice"), "/home/alice")).toBe("~/a/b");
  });
});
