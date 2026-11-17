import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import type { FoundShell, LocalShell, Profile } from "../ipc/api";
import { defaultLocal, formatArgs, parseArgs, programFile, sameShell, shellCommand, shellTitle } from "./localShells";

const t = ((key: string) => `<${key}>`) as unknown as TFunction;

const program = (path: string, ...args: string[]): LocalShell => ({ kind: "program", path, args });

describe("arguments", () => {
  it("are split at spaces and grouped by quotes", () => {
    expect(parseArgs("  --login   -i ")).toEqual(["--login", "-i"]);
    expect(parseArgs('-c "echo hi" "" x"y z"w')).toEqual(["-c", "echo hi", "", "xy zw"]);
    expect(parseArgs('"say ""hi"""')).toEqual(['say "hi"']);
    expect(parseArgs("C:\\Users\\me\\a.ps1")).toEqual(["C:\\Users\\me\\a.ps1"]);
    expect(parseArgs("")).toEqual([]);
  });

  it("read back as they were written", () => {
    for (const args of [[], ["-NoLogo"], ["--login", "-i"], ["", "a b", 'x"y', "C:\\a b\\", "tab\there"]]) {
      expect(parseArgs(formatArgs(args))).toEqual(args);
    }
    expect(formatArgs(["-c", "echo hi"])).toBe('-c "echo hi"');
  });
});

describe("local shells", () => {
  it("are the same when they run the same", () => {
    expect(sameShell(program("/bin/zsh", "-l"), program("/bin/zsh", "-l"))).toBe(true);
    expect(sameShell(program("/bin/zsh", "-l"), program("/bin/zsh"))).toBe(false);
    expect(sameShell({ kind: "wsl", distro: "Ubuntu" }, { kind: "wsl", distro: "Debian" })).toBe(false);
    expect(sameShell({ kind: "default" }, { kind: "default" })).toBe(true);
    expect(sameShell({ kind: "default" }, program("/bin/zsh"))).toBe(false);
  });

  it("are listed by what they run", () => {
    expect(programFile("C:\\Program Files\\Git\\bin\\bash.exe")).toBe("bash.exe");
    expect(shellCommand(program("C:\\Program Files\\Git\\bin\\bash.exe", "--login", "-i"))).toBe("bash.exe --login -i");
    expect(shellCommand(program("/bin/zsh"))).toBe("zsh");
    expect(shellCommand({ kind: "wsl", distro: "Ubuntu 24" })).toBe('wsl -d "Ubuntu 24"');
    expect(shellCommand({ kind: "default" })).toBe("localhost");
  });

  it("are titled by the name found, else by the program or distribution", () => {
    const found: FoundShell[] = [
      { kind: "cmd", name: "cmd", shell: program("C:\\Windows\\System32\\cmd.exe") },
      { kind: "gitBash", name: "Git Bash", shell: program("C:\\Git\\bin\\bash.exe", "--login", "-i") },
    ];
    expect(shellTitle({ kind: "default" }, found, "pwsh", t)).toBe("pwsh");
    expect(shellTitle(program("C:\\Windows\\System32\\cmd.exe"), found, "pwsh", t)).toBe("<local.cmd>");
    expect(shellTitle(program("C:\\Git\\bin\\bash.exe", "--login", "-i"), found, "pwsh", t)).toBe("Git Bash");
    expect(shellTitle(program("C:\\Git\\bin\\bash.exe"), found, "pwsh", t)).toBe("bash");
    expect(shellTitle(program("/usr/bin/fish"), [], "zsh", t)).toBe("fish");
    expect(shellTitle({ kind: "wsl", distro: "Debian" }, found, "pwsh", t)).toBe("Debian");
  });
});

describe("the default local terminal", () => {
  const session = (id: string, protocol: "local" | "telnet"): Profile =>
    ({
      id,
      name: id,
      connection:
        protocol === "local"
          ? { protocol, shell: { kind: "default" }, startDir: "", env: [], imported: false }
          : { protocol, host: "h", port: 23, username: "", jumpHosts: [], keepaliveInterval: 30, termType: "xterm" },
    }) as unknown as Profile;
  const profiles = [session("here", "local"), session("box", "telnet")];

  it("is the shell or saved local session set, else the default shell", () => {
    expect(defaultLocal(null, profiles)).toEqual({ shell: { kind: "default" } });
    expect(defaultLocal({ kind: "shell", shell: { kind: "wsl", distro: "Ubuntu" } }, profiles)).toEqual({
      shell: { kind: "wsl", distro: "Ubuntu" },
    });
    expect(defaultLocal({ kind: "session", profileId: "here" }, profiles)).toEqual({ profile: profiles[0] });
    // Deleted, or no longer local.
    expect(defaultLocal({ kind: "session", profileId: "gone" }, profiles)).toEqual({ shell: { kind: "default" } });
    expect(defaultLocal({ kind: "session", profileId: "box" }, profiles)).toEqual({ shell: { kind: "default" } });
  });
});
