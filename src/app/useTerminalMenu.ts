import { useTranslation } from "react-i18next";

import type { MenuItem } from "../components/ContextMenu";
import type { QuickCommand } from "../ipc/api";
import { useLocale } from "../hooks/locale";
import { useSettings } from "../hooks/settings";
import type { WindowInfo } from "../ipc/windows";
import type { Direction } from "../lib/layout";
import { findPane, tabTitle, type Pane, type Tab } from "../lib/panes";
import { leavesSomething, type PanePlace, type PaneSide } from "../lib/tabs";
import type { CloseRequest } from "./useCloseFlow";

/** Quick commands listed in the terminal's menu; the palette has them all. */
const MENU_COMMANDS = 8;

/** What the menu does to panes (see `usePaneActions`). */
interface PaneActions {
  roomToSplit(key: number, direction: Direction): boolean;
  splitPane(key: number, direction: Direction): void;
  /** Where a pane moved into the tab goes, or null when there is no room. */
  placeIn(tabKey: number): PaneSide | null;
  movePane(key: number, to: PanePlace): void;
}

/** The other windows, where panes can move to (see `useWindows`). */
interface Windowing {
  others: WindowInfo[];
  canMove(tab: Tab, only?: number): boolean;
  moveTab(key: number, target: string | null, options: { pane: number }): Promise<void>;
}

interface Options {
  tabs: Tab[];
  paneActions: PaneActions;
  windowing: Windowing;
  requestClose(request: CloseRequest): unknown;
  fullScreen: { on: boolean; toggle(): void };
  /** The quick commands of a pane's group, or null when the menu lists none. */
  commandsOf(pane: Pane): QuickCommand[] | null;
  runCommand(command: QuickCommand): void;
  openPalette(): void;
}

/**
 * What the app adds to a terminal's menu: splitting, closing and moving the pane, full
 * screen, then quick commands (those of the pane's group, and the palette).
 */
export function useTerminalMenu({
  tabs,
  paneActions,
  windowing,
  requestClose,
  fullScreen,
  commandsOf,
  runCommand,
  openPalette,
}: Options): (key: number) => MenuItem[] {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  // The terminal builds its menu as it renders.
  const { followRemoteTitle } = useSettings().settings.tabs;

  const paneMenu = (pane: Pane): MenuItem[] => {
    const split = (tabs.find((tab) => tab.panes.some((p) => p.key === pane.key))?.panes.length ?? 0) > 1;
    // A serial device can only be open once.
    const serial = pane.protocol === "serial";
    return [
      "separator",
      {
        label: t("tabs.splitRight"),
        shortcut: shortcutLabel("splitRight"),
        disabled: serial || !paneActions.roomToSplit(pane.key, "row"),
        onSelect: () => paneActions.splitPane(pane.key, "row"),
      },
      {
        label: t("tabs.splitDown"),
        shortcut: shortcutLabel("splitDown"),
        disabled: serial || !paneActions.roomToSplit(pane.key, "column"),
        onSelect: () => paneActions.splitPane(pane.key, "column"),
      },
      ...(split
        ? [
            {
              label: t("tabs.closePane"),
              shortcut: shortcutLabel("closeTab"),
              onSelect: () => void requestClose({ kind: "pane", key: pane.key }),
            },
          ]
        : []),
      ...moveMenu(pane),
      "separator",
      {
        label: t(fullScreen.on ? "terminal.menu.exitFullScreen" : "terminal.menu.fullScreen"),
        shortcut: shortcutLabel("fullScreen"),
        onSelect: fullScreen.toggle,
      },
    ];
  };

  /**
   * Moving the pane: to a tab of its own, or next to the focused pane of another tab (where
   * there is room), or to another window. The tabs are listed as in the tab bar.
   */
  const moveMenu = (pane: Pane): MenuItem[] => {
    const tab = findPane(tabs, pane.key)?.tab;
    const others = tabs.filter((other) => other.key !== tab?.key);
    if (!tab || (tab.panes.length === 1 && others.length === 0 && windowing.others.length === 0)) return [];
    const toWindow = windowing.canMove(tab, pane.key);
    const moveToWindow = (target: string | null) => void windowing.moveTab(tab.key, target, { pane: pane.key });
    const targets: MenuItem[] = [
      {
        label: t("tabs.paneTo.newWindow"),
        disabled: !toWindow || !leavesSomething(tabs, tab.key, pane.key),
        onSelect: () => moveToWindow(null),
      },
      ...windowing.others.map((other) => ({
        label: t("tabs.paneTo.window", { name: other.title || t("app.name") }),
        disabled: !toWindow,
        onSelect: () => moveToWindow(other.label),
      })),
    ];
    if (others.length > 0) targets.push("separator");
    for (const other of others) {
      const place = tab.moving ? null : paneActions.placeIn(other.key);
      targets.push({
        label: tabTitle(other, followRemoteTitle),
        disabled: !place,
        onSelect: () => place && paneActions.movePane(pane.key, { kind: "pane", ...place }),
      });
    }
    return [
      "separator",
      ...(tab.panes.length > 1
        ? [{ label: t("tabs.movePaneToNewTab"), disabled: !!tab.moving, onSelect: () => paneActions.movePane(pane.key, { kind: "tab" }) }]
        : []),
      { label: t("tabs.movePaneTo"), submenu: targets },
    ];
  };

  const commandMenu = (pane: Pane): MenuItem[] => {
    const commands = commandsOf(pane);
    if (!commands) return [];
    return [
      "separator",
      ...commands.slice(0, MENU_COMMANDS).map((command) => ({ label: command.name, onSelect: () => runCommand(command) })),
      { label: t("quick.paletteMenu"), shortcut: shortcutLabel("quickCommands"), onSelect: openPalette },
    ];
  };

  // By key: the pane as it is when the menu opens, so that the terminal's props don't change
  // with every change to its pane (its title, its progress).
  return (key) => {
    const pane = findPane(tabs, key)?.pane;
    return pane ? [...paneMenu(pane), ...commandMenu(pane)] : [];
  };
}
