import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import { DEFAULT_PORTS, type Profile } from "../ipc/api";
import { parsePort } from "./format";
import { fromProfile, isChanged, pagesOf, toProfile, validate, withProtocol, withShell } from "./profileForm";

const t = ((key: string) => key) as unknown as TFunction;

const saved: Profile = {
  id: "p1",
  name: "web",
  folder: "f1",
  connection: {
    protocol: "ssh",
    host: "web.example.com",
    port: 2222,
    username: "alice",
    jumpHosts: ["bastion"],
    keepaliveInterval: 15,
    termType: "xterm-256color",
    auth: { type: "publicKey", keyPath: "~/.ssh/id_ed25519" },
    forwardAgent: true,
    env: [{ name: "LANG", value: "en_US.UTF-8" }],
    forwards: [
      {
        id: "r1",
        kind: "local",
        bindHost: "localhost",
        bindPort: 8080,
        targetHost: "localhost",
        targetPort: 80,
        description: "",
        autoStart: false,
      },
    ],
  },
  autoReconnect: false,
  encoding: "utf-8",
  loginCommands: ["cd /srv"],
  appearance: { colorScheme: "nord", fontSize: 14 },
  autoLog: true,
  commandGroup: "g1",
  notes: "Web server\nAsk Bob for root",
};

describe("the session form", () => {
  it("gives back the profile it was made from", () => {
    const form = fromProfile(saved);
    expect(validate(form, t)).toBeNull();
    expect(toProfile(form, saved, undefined, "g1")).toEqual(saved);
    expect(isChanged(form, fromProfile(saved))).toBe(false);
    expect(isChanged({ ...form, name: "web2" }, form)).toBe(true);
  });

  it("starts a new session from a quick connection, in its folder", () => {
    const form = fromProfile(null, { protocol: "telnet", host: "router", port: 23, folder: "f2" });
    expect([form.protocol, form.host, form.port]).toEqual(["telnet", "router", "23"]);
    const profile = toProfile(form, null, "f2", "default");
    expect([profile.id, profile.folder, profile.commandGroup]).toEqual(["", "f2", undefined]);
  });

  it("names no key file a session doesn't use, which may not exist here", () => {
    expect(fromProfile(null).keyPath).toBe("");
    const auto: Profile = { ...saved, connection: { ...saved.connection, auth: { type: "auto" } } as Profile["connection"] };
    expect(fromProfile(auto).keyPath).toBe("");
  });

  it("says which field can't be saved, on which page", () => {
    const form = fromProfile(saved);
    expect(validate({ ...form, host: " " }, t)).toMatchObject({ page: "general", field: "host" });
    expect(validate({ ...form, port: "0x16" }, t)).toMatchObject({ page: "general", field: "port" });
    expect(validate({ ...form, keyPath: "" }, t)).toMatchObject({ field: "keyPath" });
    expect(validate({ ...form, keepalive: "4000" }, t)).toMatchObject({ page: "connection", field: "keepalive" });
    expect(validate({ ...form, env: "LANG" }, t)).toMatchObject({ page: "terminal", field: "env" });
    expect(validate({ ...form, fontSize: "99" }, t)).toMatchObject({ page: "appearance", field: "fontSize" });
    // Other protocols' fields aren't checked.
    expect(validate({ ...form, protocol: "serial", host: "", device: "COM3" }, t)).toBeNull();
    expect(validate({ ...form, protocol: "serial", device: "COM3", baudRate: "0" }, t)).toMatchObject({ field: "baudRate" });
  });

  it("moves the port with the protocol unless it was changed", () => {
    const form = fromProfile(null);
    expect(withProtocol(form, "telnet", DEFAULT_PORTS).port).toBe("23");
    expect(withProtocol({ ...form, port: "2222" }, "telnet", DEFAULT_PORTS).port).toBe("2222");
  });
});

describe("a local session's form", () => {
  const local: Profile = {
    ...saved,
    connection: {
      protocol: "local",
      shell: { kind: "program", path: "C:\\Program Files\\Git\\bin\\bash.exe", args: ["--login", "-i"] },
      startDir: "~/src",
      env: [{ name: "EDITOR", value: "vim" }],
      imported: true,
    },
  };

  it("gives back the session it was made from, still to be confirmed", () => {
    const form = fromProfile(local);
    expect(form.args).toBe("--login -i");
    expect(validate(form, t)).toBeNull();
    expect(toProfile(form, local, undefined, "g1")).toEqual(local);
  });

  it("runs what is chosen, and needs a program or a distribution", () => {
    const form = fromProfile(null, { protocol: "local", shell: { kind: "wsl", distro: "Ubuntu" } });
    expect([form.localShell, form.distro]).toEqual(["wsl", "Ubuntu"]);
    expect(validate({ ...form, distro: " " }, t)).toMatchObject({ page: "general", field: "distro" });
    const zsh = withShell(form, { kind: "program", path: "/bin/zsh", args: ["-l"] });
    expect(toProfile(zsh, null, undefined, "default").connection).toEqual({
      protocol: "local",
      shell: { kind: "program", path: "/bin/zsh", args: ["-l"] },
      startDir: "",
      env: [],
      imported: false,
    });
    expect(validate({ ...zsh, program: "" }, t)).toMatchObject({ page: "general", field: "program" });
    expect(validate({ ...zsh, env: "EDITOR" }, t)).toMatchObject({ page: "terminal", field: "env" });
    expect(toProfile(withShell(zsh, { kind: "default" }), null, undefined, "default").connection).toMatchObject({
      shell: { kind: "default" },
    });
  });

  it("has no connection page", () => {
    expect(pagesOf("local")).toEqual(["general", "terminal", "appearance"]);
    expect(pagesOf("ssh")).toContain("connection");
  });
});

describe("parsePort", () => {
  it("takes digits only, within the range", () => {
    expect([parsePort("22"), parsePort(" 65535 "), parsePort("0"), parsePort("0", { allowZero: true })]).toEqual([22, 65535, null, 0]);
    expect([parsePort("0x16"), parsePort("1e3"), parsePort("65536"), parsePort("")]).toEqual([null, null, null, null]);
  });
});
