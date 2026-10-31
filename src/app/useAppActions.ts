import { openUrl } from "@tauri-apps/plugin-opener";

import type { SectionId } from "../components/settings/common";
import { useAppLock } from "../hooks/appLock";
import { ISSUES_URL, RELEASES_URL } from "../hooks/appMenu";
import { useShortcuts } from "../hooks/shortcuts";
import { debugLog } from "../ipc/debugLog";
import { directory } from "../lib/directory";
import { matches, PANE_SIDES, TAB_ACTIONS, type Action, type MenuAction } from "../lib/keymap";
import { neighbor, type Direction } from "../lib/layout";
import { focusedPane, type SidePanel } from "../lib/panes";
import { quickCommandStore } from "../lib/quickCommands";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { activeTabOf, togglesPanel, type TabStore } from "../lib/tabs";
import { checkForUpdates } from "../lib/updates";

/** What terminal full screen hides, whose keys and menu items do nothing meanwhile. */
const HIDDEN_IN_FULL_SCREEN: readonly Action[] = ["searchSessions", "sidebar", "composeBar", "filePanel", "forwardsPanel"];

/** What an action does, when its keys are pressed or it is chosen from a menu, and when it applies. */
interface Entry {
  /** Whether it applies now (by default it always does): its menu item is off, and its keys do nothing but `refused`. */
  enabled?(): boolean;
  /** Absent for actions run where they happen (the terminal, the session list), here for their menu items. */
  run?(): void;
  /** What its keys do while it doesn't apply, rather than nothing: say why. */
  refused?(): void;
  /** Also while a dialog is open (see `useShortcuts`). */
  always?: boolean;
}

/** Splitting the focused pane (see `usePaneActions`). */
interface PaneActions {
  roomToSplit(key: number, direction: Direction): boolean;
  splitPane(key: number, direction: Direction): void;
}

interface Options {
  store: TabStore;
  sessions: SessionRegistry;
  paneActions: PaneActions;
  fullScreen: { on: boolean; toggle(): void };
  /** False on Windows in S mode, which blocks local shells. */
  localAllowed: boolean;
  openLocalTab(): void;
  /** Focuses the session list's search box, showing the list if it is hidden. */
  searchSessions(): void;
  /** Hides or shows the session list. */
  toggleSidebar(): void;
  importSshConfig(): void;
  /** Closes the focused pane or tab, or the window once it has no tabs. */
  closeFocused(): void;
  openSettings(section?: SectionId): void;
  openAbout(): void;
  openPalette(): void;
  toggleCompose(): void;
}

/**
 * The window's actions: what each does and whether it applies now, in one table for both
 * their shortcuts and the app menu, which runs them as if their keys were pressed
 * (`runAction`). Some are run where they happen and are here only for their menu items;
 * others (closing the window, the session list's) have no rule and are left out. Returns
 * whether an action of the app menu applies now.
 */
export function useAppActions(options: Options): (action: MenuAction) => boolean {
  const { store, sessions, paneActions, fullScreen } = options;
  const { lock } = useAppLock();

  const activeTab = () => activeTabOf(store.get());
  const activePane = () => {
    const tab = activeTab();
    return tab && focusedPane(tab);
  };
  const hasTabs = () => store.get().tabs.length > 0;
  const activate = (key: number) => store.dispatch({ type: "activate", key });

  /** Activates the tab at the index `next` gives for the number of tabs and the active one's index. */
  const showTab = (next: (count: number, index: number) => number) => () => {
    const { tabs, activeKey } = store.get();
    const index = next(
      tabs.length,
      tabs.findIndex((t) => t.key === activeKey),
    );
    if (index < tabs.length) activate(tabs[index].key);
  };

  const split = (direction: Direction): Entry => {
    const run = () => {
      const tab = activeTab();
      if (tab) paneActions.splitPane(tab.focused, direction);
    };
    return {
      enabled: () => {
        const pane = activePane();
        // A serial device can only be open once.
        return !!pane && pane.protocol !== "serial" && paneActions.roomToSplit(pane.key, direction);
      },
      run,
      // The pane flashes, and why is announced (see `splitPane`).
      refused: run,
    };
  };

  const panel = (kind: SidePanel): Entry => ({
    enabled: () => {
      const tab = activeTab();
      return !!tab && togglesPanel(tab, kind);
    },
    run: () => store.dispatch({ type: "togglePanel", panel: kind }),
  });

  const table: Partial<Record<Action, Entry>> = {
    // Settings and About open over a dialog too, and the app locks over one.
    lock: { always: true, run: () => !lock() && options.openSettings("security") },
    settings: { always: true, run: () => options.openSettings() },
    about: { always: true, run: options.openAbout },
    checkForUpdates: {
      always: true,
      run: () => {
        // The result shows there.
        options.openAbout();
        checkForUpdates();
      },
    },
    releaseNotes: { always: true, run: () => void openUrl(RELEASES_URL).catch(console.error) },
    reportIssue: { always: true, run: () => void openUrl(ISSUES_URL).catch(console.error) },
    debugLog: { always: true, run: debugLog.open },

    newLocalTerminal: { enabled: () => options.localAllowed, run: options.openLocalTab },
    searchSessions: { run: options.searchSessions },
    importSshConfig: { run: options.importSshConfig },
    exportSessions: { enabled: () => directory.get().profiles.length > 0 },
    closeTab: { run: options.closeFocused },

    nextTab: { enabled: hasTabs, run: showTab((count, index) => (index + 1) % count) },
    previousTab: { enabled: hasTabs, run: showTab((count, index) => (index - 1 + count) % count) },
    ...Object.fromEntries(TAB_ACTIONS.map((action, i) => [action, { enabled: hasTabs, run: showTab(() => i) }])),
    // The last tab, whatever their number, as in browsers.
    lastTab: { enabled: hasTabs, run: showTab((count) => count - 1) },
    ...Object.fromEntries(
      Object.entries(PANE_SIDES).map(([action, side]) => [
        action,
        {
          enabled: () => !!activeTab(),
          run: () => {
            const tab = activeTab()!;
            const next = neighbor(tab.layout, tab.focused, side);
            if (next !== null) store.dispatch({ type: "focusPane", key: next });
          },
        },
      ]),
    ),
    splitRight: split("row"),
    splitDown: split("column"),

    copy: {
      enabled: () => {
        const pane = activePane();
        return !!pane && sessions.hasSelection(pane.key);
      },
    },
    paste: { enabled: () => !!activePane() },
    find: { enabled: () => !!activePane() },

    filePanel: panel("files"),
    forwardsPanel: panel("forwards"),
    sidebar: { run: options.toggleSidebar },
    composeBar: { enabled: hasTabs, run: options.toggleCompose },
    // The default group is always there once the commands are read.
    quickCommands: { enabled: () => hasTabs() && quickCommandStore.get().value !== null, run: options.openPalette },
    fullScreen: { enabled: hasTabs, run: fullScreen.toggle },
  };

  const applies = (action: Action) => !(fullScreen.on && HIDDEN_IN_FULL_SCREEN.includes(action)) && (table[action]?.enabled?.() ?? true);

  const take = (e: KeyboardEvent, always: boolean) => {
    const action = (Object.keys(table) as Action[]).find((a) => !!table[a]!.run && !!table[a]!.always === always && matches(e, a));
    if (!action) return false;
    const entry = table[action]!;
    if (applies(action)) entry.run!();
    else entry.refused?.();
    return true;
  };
  useShortcuts((e) => take(e, true), { when: "always" });
  useShortcuts((e) => take(e, false));

  return applies;
}
