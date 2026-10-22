import { useState } from "react";

import { CommandPalette } from "../components/CommandPalette";
import { QuickCommandBar } from "../components/QuickCommandBar";
import type { QuickCommand } from "../ipc/api";
import { useDirectory, useQuickCommands } from "../hooks/stores";
import { asTyped, type SendResult } from "../lib/compose";
import { targetProfile, type Pane } from "../lib/panes";
import { storeBarVisible, storedBarVisible, tabGroup } from "../lib/quickCommands";
import type { TabStore } from "../lib/tabs";

interface Options {
  /** The focused pane of the active tab. */
  activePane: Pane | undefined;
  /** As for `QuickCommandBar`: the tabs a command goes to when more than the active one. */
  targets: string[] | null;
  /** Sends text as if typed to the panes the compose bar's scope says. */
  send(data: string): SendResult;
  /** Moves the keyboard focus back to the active terminal. */
  focusTerminal(): void;
}

/**
 * The quick commands: the bar under the terminals (shown or not) and the palette, each of the
 * active tab's group, and those of a pane's group for its menu. `bar` and `palette` are what
 * to render.
 */
export function useQuickCommandBar(store: TabStore, { activePane, targets, send, focusTerminal }: Options) {
  const [barOpen, setBarOpen] = useState(storedBarVisible);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const { commands } = useQuickCommands();
  const { profiles } = useDirectory();

  const toggleBar = () => {
    storeBarVisible(!barOpen);
    setBarOpen(!barOpen);
  };

  /** The group a pane shows: picked in it, else its session's (see `tabGroup`); null until the commands are read. */
  const groupOf = (pane: Pane) => commands && tabGroup(commands, pane.commandGroup, targetProfile(pane.target, profiles)?.commandGroup);
  const group = activePane ? groupOf(activePane) : null;

  /** The commands a pane's menu lists, null for none (there are none at all). */
  const menuCommands = (pane: Pane): QuickCommand[] | null => {
    const paneGroup = groupOf(pane);
    return !commands || !paneGroup || commands.groups.every((g) => g.commands.length === 0) ? null : paneGroup.commands;
  };

  const run = (command: QuickCommand) => send(asTyped(command.text, command.enter));

  const bar = barOpen && activePane && group && (
    <QuickCommandBar
      group={group}
      onPickGroup={(id) => store.dispatch({ type: "updatePane", key: activePane.key, patch: { commandGroup: id } })}
      onRun={run}
      targets={targets}
    />
  );

  const palette = paletteOpen && group && (
    <CommandPalette
      firstGroup={group.id}
      targets={targets}
      onRun={run}
      onClose={() => {
        setPaletteOpen(false);
        focusTerminal();
      }}
    />
  );

  return { barOpen, toggleBar, openPalette: () => setPaletteOpen(true), menuCommands, run, bar, palette };
}
