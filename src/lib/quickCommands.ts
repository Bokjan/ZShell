import type { TFunction } from "i18next";

import { DEFAULT_GROUP, type CommandGroup, type QuickCommand, type QuickCommands } from "./api";
import { storedString, storeString } from "./storage";

export const groupName = (group: CommandGroup, t: TFunction): string =>
  group.id === DEFAULT_GROUP ? t("quick.defaultGroup") : group.name;

/**
 * The group a tab shows: the one picked in it, else its session's, else the default group
 * (also for groups deleted since).
 */
export function tabGroup(commands: QuickCommands, picked: string | null, sessionGroup: string | undefined) {
  return (
    commands.groups.find((group) => group.id === picked) ??
    commands.groups.find((group) => group.id === sessionGroup) ??
    commands.groups[0]
  );
}

export interface Match {
  group: CommandGroup;
  command: QuickCommand;
}

/**
 * Commands whose name, text or group contain every word of `query`, those of `first` (the
 * active tab's group) before the others, each group in its order.
 */
export function searchCommands(commands: QuickCommands, query: string, first: string, t: TFunction): Match[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const groups = [...commands.groups].sort((a, b) => Number(b.id === first) - Number(a.id === first));
  return groups.flatMap((group) =>
    group.commands
      .filter((command) => {
        const text = `${command.name} ${command.text} ${groupName(group, t)}`.toLowerCase();
        return words.every((word) => text.includes(word));
      })
      .map((command) => ({ group, command })),
  );
}

const mapGroups = (commands: QuickCommands, f: (group: CommandGroup) => CommandGroup | null): QuickCommands => ({
  groups: commands.groups.map(f).filter((group): group is CommandGroup => group !== null),
});

/**
 * Adds a command (empty id) at the end of `groupId`, or updates one, moving it to the end of
 * `groupId` if it changed group.
 */
export function saveCommand(commands: QuickCommands, groupId: string, command: QuickCommand): QuickCommands {
  const current = commands.groups.find((group) => group.commands.some((c) => c.id === command.id && c.id !== ""));
  if (current?.id === groupId) {
    return mapGroups(commands, (group) =>
      group.id === groupId ? { ...group, commands: group.commands.map((c) => (c.id === command.id ? command : c)) } : group,
    );
  }
  return mapGroups(commands, (group) => {
    const rest = group.commands.filter((c) => c.id !== command.id || c.id === "");
    return group.id === groupId ? { ...group, commands: [...rest, command] } : { ...group, commands: rest };
  });
}

export const deleteCommand = (commands: QuickCommands, id: string): QuickCommands =>
  mapGroups(commands, (group) => ({ ...group, commands: group.commands.filter((c) => c.id !== id) }));

/** Moves a command to `index` within its group. */
export const moveCommand = (commands: QuickCommands, id: string, index: number): QuickCommands =>
  mapGroups(commands, (group) => {
    const command = group.commands.find((c) => c.id === id);
    if (!command) return group;
    const rest = group.commands.filter((c) => c.id !== id);
    return { ...group, commands: [...rest.slice(0, index), command, ...rest.slice(index)] };
  });

export const addGroup = (commands: QuickCommands, name: string): QuickCommands => ({
  groups: [...commands.groups, { id: "", name, commands: [] }],
});

export const renameGroup = (commands: QuickCommands, id: string, name: string): QuickCommands =>
  mapGroups(commands, (group) => (group.id === id ? { ...group, name } : group));

/** Deletes a group; its commands move to the end of the default group, so none is lost. */
export function deleteGroup(commands: QuickCommands, id: string): QuickCommands {
  const moved = commands.groups.find((group) => group.id === id)?.commands ?? [];
  return mapGroups(commands, (group) =>
    group.id === id ? null : group.id === DEFAULT_GROUP ? { ...group, commands: [...group.commands, ...moved] } : group,
  );
}

const BAR_KEY = "zshell.quickCommandBar";

/** Whether the quick command bar is shown; layout state, kept per machine like the sidebar width. */
export const storedBarVisible = () => storedString(BAR_KEY) === "1";

export const storeBarVisible = (visible: boolean) => storeString(BAR_KEY, visible ? "1" : "0");
