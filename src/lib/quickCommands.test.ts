import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import { DEFAULT_GROUP, type QuickCommand, type QuickCommands } from "./api";
import { addGroup, deleteCommand, deleteGroup, moveCommand, renameGroup, saveCommand, searchCommands, tabGroup } from "./quickCommands";

const t = ((key: string) => key) as TFunction;
const command = (id: string, name: string, text = name): QuickCommand => ({ id, name, text, enter: true });
const syslog = command("syslog", "syslog", "tail /var/log/syslog");
const deploy = command("deploy", "deploy", "./deploy.sh");
const logs = command("logs", "logs", "tail -f /var/log/app.log");
const commands: QuickCommands = {
  groups: [
    { id: DEFAULT_GROUP, name: "", commands: [syslog] },
    { id: "ops", name: "Ops", commands: [deploy, logs] },
  ],
};
const ids = (commands: QuickCommands) => Object.fromEntries(commands.groups.map((group) => [group.id, group.commands.map((c) => c.id)]));

describe("tabGroup", () => {
  it("shows the group picked in the tab, else the session's, else the default one", () => {
    expect(tabGroup(commands, DEFAULT_GROUP, "ops").id).toBe(DEFAULT_GROUP);
    expect(tabGroup(commands, null, "ops").id).toBe("ops");
    expect(tabGroup(commands, null, undefined).id).toBe(DEFAULT_GROUP);
  });

  it("falls back to the default group for groups deleted since", () => {
    expect(tabGroup(commands, "gone", "also-gone").id).toBe(DEFAULT_GROUP);
  });
});

describe("searchCommands", () => {
  it("finds commands by name, text or group, the given group's first", () => {
    const found = (query: string, first: string) => searchCommands(commands, query, first, t).map((match) => match.command.id);
    expect(found("log", "ops")).toEqual(["logs", "syslog"]);
    expect(found("log", DEFAULT_GROUP)).toEqual(["syslog", "logs"]);
    expect(found("tail APP", DEFAULT_GROUP)).toEqual(["logs"]);
    expect(found("ops", DEFAULT_GROUP)).toEqual(["deploy", "logs"]);
  });
});

describe("editing", () => {
  it("adds a new command at the end of its group", () => {
    expect(ids(saveCommand(commands, "ops", command("", "new")))).toEqual({ default: ["syslog"], ops: ["deploy", "logs", ""] });
  });

  it("updates a command in place, or moves it to the end of the group it changed to", () => {
    const renamed = saveCommand(commands, "ops", { ...deploy, name: "ship" });
    expect(renamed.groups[1].commands.map((c) => c.name)).toEqual(["ship", "logs"]);
    expect(ids(saveCommand(commands, DEFAULT_GROUP, deploy))).toEqual({ default: ["syslog", "deploy"], ops: ["logs"] });
  });

  it("deletes and moves commands", () => {
    expect(ids(deleteCommand(commands, "deploy"))).toEqual({ default: ["syslog"], ops: ["logs"] });
    expect(ids(moveCommand(commands, "logs", 0))).toEqual({ default: ["syslog"], ops: ["logs", "deploy"] });
  });

  it("adds and renames groups", () => {
    expect(addGroup(commands, "Db").groups.map((group) => [group.id, group.name])).toEqual([
      [DEFAULT_GROUP, ""],
      ["ops", "Ops"],
      ["", "Db"],
    ]);
    expect(renameGroup(commands, "ops", "Prod").groups[1].name).toBe("Prod");
  });

  it("moves the commands of a deleted group to the end of the default one", () => {
    expect(ids(deleteGroup(commands, "ops"))).toEqual({ default: ["syslog", "deploy", "logs"] });
  });
});
