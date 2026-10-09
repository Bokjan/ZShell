import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { CommandPalette } from "./components/CommandPalette";
import { ComposeBar } from "./components/ComposeBar";
import type { MenuItem } from "./components/ContextMenu";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ImportDialog } from "./components/ImportDialog";
import { ProfileDialog, type ProfileDefaults } from "./components/ProfileDialog";
import { QuickCommandBar } from "./components/QuickCommandBar";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar, type OpenMode } from "./components/Sidebar";
import { PANEL_SHORTCUTS, TabBar } from "./components/TabBar";
import { MIN_PANE_HEIGHT, MIN_PANE_WIDTH, TabPage, type PaneHandlers } from "./components/TabPage";
import type { SessionStatus } from "./components/TerminalView";
import { Tooltips } from "./components/Tooltip";
import {
  listProfiles,
  localShellName,
  localUsername,
  quickCommands,
  sendBreak,
  sessionLog,
  sessionForeground,
  tree,
  writeSession,
  type Folder,
  type ForwardState,
  type LogOpen,
  type Profile,
  type QuickCommand,
  type QuickCommands,
  type SessionId,
  type SessionTarget,
} from "./lib/api";
import {
  closeTabShortcutLabel,
  hasShiftShortcutModifiers,
  isNewTabShortcut,
  isSearchShortcut,
  isSettingsShortcut,
  paneFocusShortcut,
  shiftShortcutLabel,
  splitDownShortcutLabel,
  splitRightShortcutLabel,
  splitShortcut,
  tabShortcut,
} from "./lib/platform";
import { heirOf, neighbor, removeFromLayout, splitLayout, type Direction } from "./lib/layout";
import { storeBarVisible, storedBarVisible, tabGroup } from "./lib/quickCommands";
import { asTyped, CLOSED_COMPOSE, isConnected, scopePanes, sendsToMany, type Compose, type SendResult } from "./lib/compose";
import {
  findPane,
  focusedPane,
  paneLabel,
  tabTitle,
  type Pane,
  type SidePanel,
  type Tab,
  type TabProtocol,
} from "./lib/panes";
import { addRecent, address, sessionsIn, storedRecent, type QuickTarget } from "./lib/sessions";
import { useSettings } from "./lib/settings";
import { tabMark } from "./lib/terminalSchemes";
import { useTitleBar } from "./lib/window";
import "./styles.css";

const profileIdOf = (pane: Pane) => (pane.target.kind === "profile" ? pane.target.profileId : undefined);

/** Why closing a pane needs confirmation: a remote session is connected, or a local program runs. */
type Busy = { kind: "connected" } | { kind: "process"; name: string };

interface Closing {
  panes: number[];
  tabs: number;
  busy: Busy;
  /** Of the busy pane. */
  name: string;
}

/** Opening more sessions than this at once (a folder) asks first. */
const OPEN_ALL_CONFIRM = 5;
/** Quick commands listed in the terminal's menu; the palette has them all. */
const MENU_COMMANDS = 8;
/** How long panes that received text from the compose bar flash. */
const FLASH_MS = 700;

/** Moves the keyboard focus back to the active terminal (after the compose bar closes). */
const focusActiveTerminal = () =>
  requestAnimationFrame(() =>
    document.querySelector<HTMLElement>(".tab-page.active .pane.focused .xterm-helper-textarea")?.focus(),
  );

async function busyReason(pane: Pane): Promise<Busy | null> {
  if (pane.status !== "connected" || pane.sessionId == null) return null;
  if (pane.target.kind !== "local") return { kind: "connected" };
  const name = await sessionForeground(pane.sessionId).catch(() => null);
  return name === null ? null : { kind: "process", name };
}

const newPane = (key: number, target: SessionTarget, protocol: TabProtocol, title: string, shareFrom?: SessionId): Pane => ({
  key,
  target,
  protocol,
  title,
  remoteTitle: null,
  status: "connecting",
  sessionId: null,
  shareFrom,
  reconnectKey: 0,
  forwards: {},
  commandGroup: null,
  logPath: null,
  logStopped: false,
});

const newTab = (key: number, pane: Pane): Tab => ({
  key,
  layout: { kind: "pane", key: pane.key },
  panes: [pane],
  focused: pane.key,
  customTitle: null,
  sidePanel: null,
});

/** How a pane's next session starts logging (see `Pane.logPath`). */
const logOpen = (pane: Pane): LogOpen =>
  pane.logPath ? { mode: "append", path: pane.logPath } : pane.logStopped ? { mode: "off" } : { mode: "auto" };

function App() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  useTitleBar();
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [recent, setRecent] = useState(storedRecent);
  const [searchFocusKey, setSearchFocusKey] = useState(0);
  // A folder's sessions waiting for confirmation to open them all.
  const [openingAll, setOpeningAll] = useState<{ folder: Folder; profiles: Profile[] } | null>(null);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeKey, setActiveKey] = useState<number | null>(null);
  const [compose, setCompose] = useState<Compose>(CLOSED_COMPOSE);
  const composeRef = useRef(compose);
  composeRef.current = compose;
  const [flashing, setFlashing] = useState<number[]>([]);
  const [commands, setCommands] = useState<QuickCommands | null>(null);
  const [quickBarOpen, setQuickBarOpen] = useState(storedBarVisible);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const latestCommands = useRef(0);
  const flashTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  // The profile dialog: an existing profile, or a new one (null) with defaults; `paneKey` is
  // the quick connection pane being saved as a session.
  const [editing, setEditing] = useState<{ profile: Profile | null; defaults?: ProfileDefaults; paneKey?: number } | null>(
    null,
  );
  const [importing, setImporting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Panes waiting for the user to confirm closing them, with why the first busy one is busy;
  // `tabs` is how many tabs are being closed, 0 for a pane.
  const [closing, setClosing] = useState<Closing | null>(null);
  // The window waiting for the user to confirm closing it (quitting), with how many tabs are busy.
  const [closingWindow, setClosingWindow] = useState<number | null>(null);
  // Title for local terminal tabs, e.g. "zsh".
  const [shellName, setShellName] = useState<string | null>(null);
  // False on Windows in S mode, which blocks local shells.
  const [localAllowed, setLocalAllowed] = useState(true);
  // For quick connections without a user name.
  const [username, setUsername] = useState("");
  // For tabs and panes alike.
  const nextKey = useRef(1);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const profilesRef = useRef(profiles);
  profilesRef.current = profiles;
  const activeKeyRef = useRef(activeKey);
  activeKeyRef.current = activeKey;
  const localTitleRef = useRef("");
  localTitleRef.current = shellName ?? t("tabs.localTitle");

  const reloadProfiles = useCallback(() => {
    listProfiles().then(setProfiles).catch(console.error);
    tree.folders().then(setFolders).catch(console.error);
  }, []);
  useEffect(reloadProfiles, [reloadProfiles]);
  useEffect(() => {
    quickCommands.get().then(setCommands).catch(console.error);
  }, []);

  // Applied immediately; the stored copy (with ids for new commands) replaces it unless a
  // newer change was made in the meantime.
  const saveCommands = (next: QuickCommands) => {
    const request = ++latestCommands.current;
    setCommands(next);
    quickCommands.set(next).then((saved) => request === latestCommands.current && setCommands(saved), console.error);
  };

  const toggleQuickBar = () => {
    storeBarVisible(!quickBarOpen);
    setQuickBarOpen(!quickBarOpen);
  };
  useEffect(() => {
    localShellName()
      .then((name) => (name === null ? setLocalAllowed(false) : setShellName(name)))
      .catch(console.error);
    localUsername().then(setUsername).catch(console.error);
  }, []);

  /** What a tab's next session will be: a saved session's protocol may have been changed. */
  const protocolOf = useCallback((target: SessionTarget): TabProtocol => {
    if (target.kind === "local") return "local";
    if (target.kind === "quick") return target.protocol;
    return profilesRef.current.find((p) => p.id === target.profileId)?.protocol ?? "ssh";
  }, []);

  const addTab = useCallback(
    (target: SessionTarget, title: string) => {
      const pane = newPane(nextKey.current++, target, protocolOf(target), title);
      const key = nextKey.current++;
      setTabs((tabs) => [...tabs, newTab(key, pane)]);
      setActiveKey(key);
    },
    [protocolOf],
  );

  /** Shows the pane: activates its tab and focuses it there. */
  const focusPane = useCallback((key: number) => {
    const found = findPane(tabsRef.current, key);
    if (!found) return;
    setActiveKey(found.tab.key);
    if (found.tab.focused !== key) {
      setTabs((tabs) => tabs.map((t) => (t.key === found.tab.key ? { ...t, focused: key } : t)));
    }
  }, []);

  /**
   * "connect" switches to a pane the session already has; "newTab" always opens a tab;
   * "splitRight" and "splitDown" split the focused pane (or open a tab if there is none).
   */
  const openProfile = (profile: Profile, mode: OpenMode) => {
    setRecent(addRecent(profile.id));
    const target: SessionTarget = { kind: "profile", profileId: profile.id };
    const existing = tabsRef.current
      .flatMap((t) => t.panes)
      .find((p) => p.target.kind === "profile" && p.target.profileId === profile.id);
    const active = tabsRef.current.find((t) => t.key === activeKeyRef.current);
    if (mode === "connect" && existing) focusPane(existing.key);
    else if ((mode === "splitRight" || mode === "splitDown") && active) {
      splitPane(active.focused, mode === "splitRight" ? "row" : "column", () =>
        newPane(nextKey.current++, target, protocolOf(target), profile.name),
      );
    } else addTab(target, profile.name);
  };

  const openAll = (folder: Folder) => {
    const list = sessionsIn(folder.id, folders, profiles);
    if (list.length > OPEN_ALL_CONFIRM) setOpeningAll({ folder, profiles: list });
    else list.forEach((p) => openProfile(p, "newTab"));
  };

  // SSH without a user name logs in as the local user.
  const quickConnect = (target: QuickTarget) =>
    addTab(
      { kind: "quick", ...target },
      address({ ...target, username: target.username || (target.protocol === "ssh" ? username : "") }),
    );

  const openLocalTab = useCallback(() => addTab({ kind: "local" }, localTitleRef.current), [addTab]);

  /**
   * A new pane running what `pane` runs: an SSH pane whose shell is up opens its copy on the
   * same connection; anything else opens the same target anew.
   */
  const copyOf = (pane: Pane): Pane => {
    const shareFrom = pane.protocol === "ssh" && pane.status === "connected" ? (pane.sessionId ?? undefined) : undefined;
    return newPane(nextKey.current++, pane.target, protocolOf(pane.target), pane.title, shareFrom);
  };

  // Copies the focused pane into a new tab. A serial device can only be open once.
  const duplicateTab = (key: number) => {
    const tabs = tabsRef.current;
    const index = tabs.findIndex((t) => t.key === key);
    if (index < 0) return;
    const pane = focusedPane(tabs[index]);
    if (pane.protocol === "serial") return;
    const copy = newTab(nextKey.current++, copyOf(pane));
    setTabs([...tabs.slice(0, index + 1), copy, ...tabs.slice(index + 1)]);
    setActiveKey(copy.key);
  };

  /**
   * Splits a pane, opening `added` (by default a copy of the pane) right of it (`row`) or
   * below it (`column`), and focuses it. Not for a pane too small to split, nor to copy a
   * serial pane (a device can only be open once).
   */
  const splitPane = (key: number, direction: Direction, added?: () => Pane) => {
    const found = findPane(tabsRef.current, key);
    if (!found || (!added && found.pane.protocol === "serial")) return;
    const rect = document.querySelector(`.pane[data-pane="${key}"]`)?.getBoundingClientRect();
    if (rect && (direction === "row" ? rect.width < 2 * MIN_PANE_WIDTH : rect.height < 2 * MIN_PANE_HEIGHT)) return;
    const pane = added ? added() : copyOf(found.pane);
    setTabs((tabs) =>
      tabs.map((t) =>
        t.key === found.tab.key
          ? { ...t, panes: [...t.panes, pane], layout: splitLayout(t.layout, key, direction, pane.key), focused: pane.key }
          : t,
      ),
    );
    setActiveKey(found.tab.key);
  };
  const splitRef = useRef(splitPane);
  splitRef.current = splitPane;

  /**
   * Closes the panes, and the tabs left without any. A pane's space goes to a neighbor, which
   * gets the focus if the closed pane had it (see `removeFromLayout`).
   */
  const closePanes = useCallback((keys: number[]) => {
    const tabs = tabsRef.current;
    const remaining = tabs.flatMap((tab) => {
      const closed = tab.panes.filter((p) => keys.includes(p.key));
      if (closed.length === tab.panes.length) return [];
      if (closed.length === 0) return [tab];
      let { layout, focused } = tab;
      for (const pane of closed) {
        if (focused === pane.key) focused = heirOf(layout, pane.key) ?? focused;
        layout = removeFromLayout(layout, pane.key)!;
      }
      return [{ ...tab, layout, focused, panes: tab.panes.filter((p) => !keys.includes(p.key)) }];
    });
    setTabs(remaining);
    const gone = (tab: Tab) => !remaining.some((t) => t.key === tab.key);
    const active = tabs.findIndex((t) => t.key === activeKeyRef.current);
    if (active >= 0 && gone(tabs[active])) {
      // The next remaining tab to the right, else the nearest one to the left.
      const next = tabs.slice(active).find((t) => !gone(t)) ?? remaining[remaining.length - 1];
      setActiveKey(next?.key ?? null);
    }
  }, []);

  /**
   * Closes tabs (`kind: "tabs"`) or a pane, first asking if any pane is connected or running
   * a program (see the settings).
   */
  const requestClose = useCallback(async (request: { kind: "tabs"; keys: number[] } | { kind: "pane"; key: number }) => {
    const panes =
      request.kind === "tabs"
        ? tabsRef.current.filter((t) => request.keys.includes(t.key)).flatMap((tab) => tab.panes.map((pane) => ({ tab, pane })))
        : [findPane(tabsRef.current, request.key)].filter((found) => found !== null);
    const keys = panes.map(({ pane }) => pane.key);
    if (settingsRef.current.tabs.confirmClose) {
      const reasons = await Promise.all(panes.map(({ pane }) => busyReason(pane)));
      const index = reasons.findIndex((reason) => reason !== null);
      if (index >= 0) {
        const { tab, pane } = panes[index];
        const follow = settingsRef.current.tabs.followRemoteTitle;
        // A split tab is named as a whole (see `closeMessage`).
        const name = request.kind === "tabs" && tab.panes.length > 1 ? tabTitle(tab, follow) : paneLabel(tab, pane, follow);
        const tabs = request.kind === "tabs" ? request.keys.length : 0;
        setClosing({ panes: keys, tabs, busy: reasons[index]!, name });
        return;
      }
    }
    closePanes(keys);
  }, [closePanes]);

  /** ⌘W / Ctrl+Shift+W: the focused pane of a split tab, otherwise the tab (the window if there is none). */
  const closeFocused = useCallback(() => {
    const tab = tabsRef.current.find((t) => t.key === activeKeyRef.current);
    if (!tab) void getCurrentWindow().close();
    else if (tab.panes.length > 1) void requestClose({ kind: "pane", key: tab.focused });
    else void requestClose({ kind: "tabs", keys: [tab.key] });
  }, [requestClose]);

  const confirmClose = (dontAskAgain: boolean) => {
    if (!closing) return;
    if (dontAskAgain) update({ ...settings, tabs: { ...settings.tabs, confirmClose: false } });
    closePanes(closing.panes);
    setClosing(null);
  };

  const cancelClose = useCallback(() => setClosing(null), []);

  // Closing the window (the close button, ⌘Q, Alt+F4) quits, ending every session at once,
  // so it always asks, whatever the setting for closing tabs.
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      const reasons = await Promise.all(tabsRef.current.flatMap((tab) => tab.panes).map(busyReason));
      const busy = reasons.filter((reason) => reason !== null).length;
      if (busy === 0) return;
      event.preventDefault();
      setClosingWindow(busy);
    });
    return () => void unlisten.then((f) => f());
  }, []);

  const confirmCloseWindow = () => void getCurrentWindow().destroy();

  const cancelCloseWindow = useCallback(() => setClosingWindow(null), []);

  const moveTab = (key: number, index: number) =>
    setTabs((tabs) => {
      const tab = tabs.find((t) => t.key === key);
      if (!tab) return tabs;
      const rest = tabs.filter((t) => t.key !== key);
      return [...rest.slice(0, index), tab, ...rest.slice(index)];
    });

  /** Runs `f` on the focused pane of the tab with this key (tab menu actions). */
  const withFocused = (key: number, f: (pane: Pane) => unknown) => {
    const pane = focusedOf(key);
    if (pane) f(pane);
  };

  const updateTab = (key: number, patch: Partial<Tab>) =>
    setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, ...patch } : t)));

  /** Changes a pane, wherever it is: `patch` may be a function of the pane and its tab. */
  const updatePane = (key: number, patch: Partial<Pane> | ((pane: Pane, tab: Tab) => Partial<Pane>)) =>
    setTabs((tabs) =>
      tabs.map((tab) =>
        tab.panes.some((p) => p.key === key)
          ? {
              ...tab,
              panes: tab.panes.map((p) => (p.key === key ? { ...p, ...(typeof patch === "function" ? patch(p, tab) : patch) } : p)),
            }
          : tab,
      ),
    );

  /** The focused pane of a tab, by the tab's key. */
  const focusedOf = (key: number) => {
    const tab = tabsRef.current.find((t) => t.key === key);
    return tab && focusedPane(tab);
  };

  // Closing the bar turns syncing off; the scope stays for the next time, shown on the bar.
  const toggleCompose = useCallback(() => {
    const open = !composeRef.current.open;
    setCompose({ ...composeRef.current, open, sync: false });
    if (!open) focusActiveTerminal();
  }, []);

  /** Names of panes, as listed for the compose bar and quick commands. */
  const labelsOf = (panes: Pane[]) =>
    panes.flatMap((pane) => {
      const found = findPane(tabsRef.current, pane.key);
      return found ? [paneLabel(found.tab, pane, settingsRef.current.tabs.followRemoteTitle)] : [];
    });

  /** Sends text as if typed to the panes in scope (see `scopePanes`); panes beyond the focused one flash. */
  const sendToScope = (data: string): SendResult => {
    const targets = scopePanes(composeRef.current, tabsRef.current, activeKeyRef.current);
    const sent = targets.filter(isConnected);
    for (const pane of sent) writeSession(pane.sessionId!, data).catch(console.error);
    if (sendsToMany(composeRef.current) && sent.length > 0) {
      clearTimeout(flashTimer.current);
      setFlashing(sent.map((pane) => pane.key));
      flashTimer.current = setTimeout(() => setFlashing([]), FLASH_MS);
    }
    return { sent: labelsOf(sent), skipped: targets.length - sent.length };
  };

  // Syncing: what is typed in the focused terminal goes to the other connected panes in scope.
  const onInput = (source: number, data: string) => {
    const compose = composeRef.current;
    const active = tabsRef.current.find((t) => t.key === activeKeyRef.current);
    if (!compose.open || !compose.sync || !active || active.focused !== source) return;
    for (const pane of scopePanes(compose, tabsRef.current, activeKeyRef.current)) {
      if (pane.key !== source && isConnected(pane)) writeSession(pane.sessionId!, data).catch(console.error);
    }
  };

  // Opens a panel only for an SSH pane; closes it whatever the pane.
  const togglePanel = useCallback((panel: SidePanel) => {
    setTabs((tabs) =>
      tabs.map((t) => {
        if (t.key !== activeKeyRef.current) return t;
        if (t.sidePanel === panel) return { ...t, sidePanel: null };
        return focusedPane(t).protocol === "ssh" ? { ...t, sidePanel: panel } : t;
      }),
    );
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isSettingsShortcut(e)) {
        e.preventDefault();
        e.stopPropagation();
        setSettingsOpen(true);
        return;
      }
      if (isNewTabShortcut(e) && localAllowed) {
        e.preventDefault();
        e.stopPropagation();
        openLocalTab();
        return;
      }
      if (isSearchShortcut(e)) {
        e.preventDefault();
        e.stopPropagation();
        setSearchFocusKey((key) => key + 1);
        return;
      }
      const tabKey = tabShortcut(e);
      if (tabKey) {
        e.preventDefault();
        e.stopPropagation();
        const tabs = tabsRef.current;
        const index = tabs.findIndex((t) => t.key === activeKeyRef.current);
        if (tabs.length === 0) return;
        if (tabKey.type === "close") {
          closeFocused();
          return;
        }
        let next: number;
        if (tabKey.type === "next") next = (index + 1) % tabs.length;
        else if (tabKey.type === "previous") next = (index - 1 + tabs.length) % tabs.length;
        // ⌘9 / Alt+9 is always the last tab, as in browsers.
        else next = tabKey.index === 8 ? tabs.length - 1 : tabKey.index;
        if (next < tabs.length) setActiveKey(tabs[next].key);
        return;
      }
      const split = splitShortcut(e);
      const side = paneFocusShortcut(e);
      if ((split || side) && activeKeyRef.current !== null) {
        e.preventDefault();
        e.stopPropagation();
        const tab = tabsRef.current.find((t) => t.key === activeKeyRef.current);
        if (!tab) return;
        if (split) splitRef.current(tab.focused, split);
        else {
          const next = neighbor(tab.layout, tab.focused, side!);
          if (next !== null) focusPane(next);
        }
        return;
      }
      if (e.code === "KeyJ" && hasShiftShortcutModifiers(e)) {
        e.preventDefault();
        e.stopPropagation();
        if (tabsRef.current.length > 0) setPaletteOpen(true);
        return;
      }
      if (e.code === "KeyI" && hasShiftShortcutModifiers(e)) {
        e.preventDefault();
        e.stopPropagation();
        if (tabsRef.current.length > 0) toggleCompose();
        return;
      }
      const panel = PANEL_SHORTCUTS[e.code];
      if (!panel || !hasShiftShortcutModifiers(e)) return;
      // Capture phase, so the terminal never sees the keystroke.
      e.preventDefault();
      e.stopPropagation();
      togglePanel(panel);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [togglePanel, closeFocused, focusPane, openLocalTab, toggleCompose, localAllowed]);

  // From the macOS app menu's "Settings…" item (⌘,).
  useEffect(() => {
    const unlisten = listen("open-settings", () => setSettingsOpen(true));
    return () => void unlisten.then((f) => f());
  }, []);

  // From the macOS File menu's "Close" item (⌘W), which closes the window once no tabs are
  // left, as in Terminal.app.
  useEffect(() => {
    const unlisten = listen("close-tab", closeFocused);
    return () => void unlisten.then((f) => f());
  }, [closeFocused]);

  const runCommand = (command: QuickCommand) => sendToScope(asTyped(command.text, command.enter));

  const activeTab = tabs.find((tab) => tab.key === activeKey);
  const activePane = activeTab && focusedPane(activeTab);
  const profileOf = (pane: Pane) => profiles.find((p) => p.id === profileIdOf(pane));
  const groupOf = (pane: Pane) => commands && tabGroup(commands, pane.commandGroup, profileOf(pane)?.commandGroup);
  const activeGroup = activePane ? groupOf(activePane) : null;
  const inScope = sendsToMany(compose) ? scopePanes(compose, tabs, activeKey) : [];
  // Who a quick command goes to, when that is more than the focused pane.
  const commandTargets = sendsToMany(compose) ? labelsOf(inScope) : null;
  /** Tabs with any of these panes. */
  const tabsWith = (keys: number[]) => tabs.filter((t) => t.panes.some((p) => keys.includes(p.key))).map((t) => t.key);

  /** Splitting and closing the pane, then quick commands: those of the pane's group, and the palette. */
  const terminalMenu = (pane: Pane): MenuItem[] => [...paneMenu(pane), ...commandMenu(pane)];

  const paneMenu = (pane: Pane): MenuItem[] => {
    const split = (tabs.find((tab) => tab.panes.some((p) => p.key === pane.key))?.panes.length ?? 0) > 1;
    // A serial device can only be open once.
    const disabled = pane.protocol === "serial";
    return [
      "separator",
      { label: t("tabs.splitRight"), shortcut: splitRightShortcutLabel, disabled, onSelect: () => splitPane(pane.key, "row") },
      { label: t("tabs.splitDown"), shortcut: splitDownShortcutLabel, disabled, onSelect: () => splitPane(pane.key, "column") },
      ...(split
        ? [{ label: t("tabs.closePane"), shortcut: closeTabShortcutLabel, onSelect: () => void requestClose({ kind: "pane", key: pane.key }) }]
        : []),
    ];
  };

  const commandMenu = (pane: Pane): MenuItem[] => {
    const group = groupOf(pane);
    if (!commands || !group || commands.groups.every((g) => g.commands.length === 0)) return [];
    return [
      "separator",
      ...group.commands.slice(0, MENU_COMMANDS).map((command) => ({ label: command.name, onSelect: () => runCommand(command) })),
      { label: t("quick.paletteMenu"), shortcut: shiftShortcutLabel("J"), onSelect: () => setPaletteOpen(true) },
    ];
  };

  /** Starts or stops logging a pane's session. */
  const setLogging = (key: number, start: boolean) => {
    const pane = findPane(tabsRef.current, key)?.pane;
    if (pane?.sessionId == null) return;
    // The session reports the path (or an error) with a `log` event.
    if (start) {
      updatePane(key, { logStopped: false });
      sessionLog.start(pane.sessionId).catch(console.error);
    } else {
      updatePane(key, { logStopped: true });
      sessionLog.stop(pane.sessionId).catch(console.error);
    }
  };

  const showLog = (key: number) => {
    const path = findPane(tabsRef.current, key)?.pane.logPath;
    if (path) revealItemInDir(path).catch(console.error);
  };

  const onStatus = (key: number, status: SessionStatus) => {
    // Forward events can precede "connected" (auto-start runs right after authentication),
    // so states are only reset when a connection attempt starts or ends.
    if (status === "connected") updatePane(key, { status });
    // A new shell sets its own title. The session's protocol may have been changed since,
    // which closes the side panel it no longer has.
    else if (status === "connecting") {
      setTabs((tabs) =>
        tabs.map((tab) => {
          if (!tab.panes.some((p) => p.key === key)) return tab;
          const panes = tab.panes.map((p) =>
            p.key === key
              ? { ...p, status, forwards: {}, remoteTitle: null, protocol: protocolOf(p.target) }
              : p,
          );
          const sidePanel = tab.focused === key && protocolOf(focusedPane(tab).target) !== "ssh" ? null : tab.sidePanel;
          return { ...tab, panes, sidePanel };
        }),
      );
    } else updatePane(key, { status, forwards: {} });
  };

  // A local shell that exits cleanly (`exit`, Ctrl+D) closes its pane, like Terminal.app.
  const onExited = (key: number, status: number | null) => {
    const found = findPane(tabsRef.current, key);
    if (found?.pane.target.kind === "local" && status === 0) closePanes([key]);
  };

  const onForward = (key: number, ruleId: string, state: ForwardState) =>
    updatePane(key, (pane) => ({ forwards: { ...pane.forwards, [ruleId]: state } }));

  const onProfileChanged = (updated: Profile) =>
    setProfiles((profiles) => profiles.map((p) => (p.id === updated.id ? updated : p)));

  const handlers: PaneHandlers = {
    onStatus,
    onExited,
    onSession: (key, sessionId) => updatePane(key, { sessionId }),
    onForward,
    onTitle: (key, title) => updatePane(key, { remoteTitle: title || null }),
    onInput,
    onLog: (key, path) => updatePane(key, { logPath: path }),
    onFocus: (key) => {
      const found = findPane(tabsRef.current, key);
      if (found && found.tab.focused !== key) updateTab(found.tab.key, { focused: key });
    },
    onLayout: (tabKey, layout) => updateTab(tabKey, { layout }),
    menuItems: terminalMenu,
    logOpen,
    profileOf,
    onProfileChanged,
  };

  const closeMessage = ({ panes, tabs, busy, name }: Closing) => {
    if (panes.length > 1 && tabs > 1) return t("closeConfirm.many");
    // A tab's panes are named; a lone pane or tab ends just its session.
    if (panes.length > 1) return t("closeConfirm.panes", { name });
    const what = tabs === 0 ? "pane" : "tab";
    if (busy.kind === "connected") return t(`closeConfirm.connected.${what}`, { name });
    return busy.name
      ? t(`closeConfirm.process.${what}`, { process: busy.name, name })
      : t(`closeConfirm.processUnknown.${what}`, { name });
  };

  const closeDialog = useCallback(() => setEditing(null), []);

  // A quick connection saved as a session becomes that session's pane; the connection stays.
  const onProfileSaved = (profile: Profile) => {
    const paneKey = editing?.paneKey;
    if (paneKey === undefined) return;
    updatePane(paneKey, { target: { kind: "profile", profileId: profile.id }, title: profile.name });
  };

  const saveAsSession = (key: number) => {
    const pane = findPane(tabsRef.current, key)?.pane;
    if (pane?.target.kind !== "quick") return;
    const { protocol, host, port } = pane.target;
    const user = pane.target.username || (protocol === "ssh" ? username : "");
    setEditing({ profile: null, defaults: { protocol, host, port, username: user }, paneKey: key });
  };
  const closeImport = useCallback(() => setImporting(false), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);

  return (
    <div className="app">
      <Sidebar
        profiles={profiles}
        folders={folders}
        recent={settings.sidebar.showRecent ? recent : []}
        searchFocusKey={searchFocusKey}
        onOpen={openProfile}
        onOpenAll={openAll}
        onQuickConnect={quickConnect}
        onEdit={(profile) => setEditing({ profile })}
        onNew={(folder) => setEditing({ profile: null, defaults: { folder } })}
        onChanged={reloadProfiles}
        onImportSshConfig={() => setImporting(true)}
        onSettings={() => setSettingsOpen(true)}
      />
      <main>
        <TabBar
          tabs={tabs}
          activeKey={activeKey}
          followRemoteTitle={settings.tabs.followRemoteTitle}
          onSelect={setActiveKey}
          onNew={localAllowed ? openLocalTab : undefined}
          onClose={(keys) => void requestClose({ kind: "tabs", keys })}
          onSplit={(key, direction) => withFocused(key, (pane) => splitPane(pane.key, direction))}
          onMove={moveTab}
          onRename={(key, customTitle) => updateTab(key, { customTitle })}
          onDuplicate={duplicateTab}
          onSaveAsSession={(key) => withFocused(key, (pane) => saveAsSession(pane.key))}
          onReconnect={(key) => withFocused(key, (pane) => updatePane(pane.key, { reconnectKey: pane.reconnectKey + 1 }))}
          onBreak={(key) =>
            withFocused(key, (pane) => pane.sessionId != null && sendBreak(pane.sessionId).catch(console.error))
          }
          onLog={(key, start) => withFocused(key, (pane) => setLogging(pane.key, start))}
          onShowLog={(key) => withFocused(key, (pane) => showLog(pane.key))}
          onTogglePanel={togglePanel}
          composeOpen={compose.open}
          onToggleCompose={toggleCompose}
          quickBarOpen={quickBarOpen}
          onToggleQuickBar={toggleQuickBar}
          inScope={tabsWith(inScope.map((pane) => pane.key))}
          syncing={compose.open && compose.sync}
          flashing={tabsWith(flashing)}
          addressOf={(tab) => {
            const pane = focusedPane(tab);
            if (pane.target.kind === "local") return undefined;
            const profile = profileOf(pane);
            return pane.target.kind === "quick" ? address(pane.target) : profile && address(profile);
          }}
          colorOf={(tab) => {
            const background = profileOf(focusedPane(tab))?.appearance?.background;
            return background && tabMark(background);
          }}
        />
        {compose.open && tabs.length > 0 && (
          <ComposeBar
            compose={compose}
            tabs={tabs}
            activeKey={activeKey}
            followRemoteTitle={settings.tabs.followRemoteTitle}
            onChange={setCompose}
            onSend={(text) => sendToScope(asTyped(text, true))}
            onClose={toggleCompose}
          />
        )}
        <div className="terminals">
          {tabs.map((tab) => (
            <TabPage
              key={tab.key}
              tab={tab}
              active={tab.key === activeKey}
              syncing={compose.open && compose.sync && tab.key === activeKey ? tab.focused : null}
              inScope={inScope.map((pane) => pane.key)}
              flashing={flashing}
              handlers={handlers}
            />
          ))}
          {tabs.length === 0 && <div className="placeholder">{t(localAllowed ? "app.placeholder" : "app.placeholderNoLocal")}</div>}
        </div>
        {quickBarOpen && activePane && commands && activeGroup && (
          <QuickCommandBar
            commands={commands}
            group={activeGroup}
            onPickGroup={(id) => updatePane(activePane.key, { commandGroup: id })}
            onChange={saveCommands}
            onRun={runCommand}
            targets={commandTargets}
          />
        )}
      </main>
      {editing && (
        <ProfileDialog
          profile={editing.profile}
          defaults={editing.defaults}
          profiles={profiles}
          commandGroups={commands?.groups ?? []}
          onClose={closeDialog}
          onChanged={reloadProfiles}
          onSaved={onProfileSaved}
        />
      )}
      {openingAll && (
        <ConfirmDialog
          title={t("sidebar.openAllTitle", { count: openingAll.profiles.length })}
          message={t("sidebar.openAllMessage", { count: openingAll.profiles.length, folder: openingAll.folder.name })}
          confirmLabel={t("sidebar.openAllConfirm")}
          onConfirm={() => {
            openingAll.profiles.forEach((p) => openProfile(p, "newTab"));
            setOpeningAll(null);
          }}
          onCancel={() => setOpeningAll(null)}
        />
      )}
      {paletteOpen && commands && activeGroup && (
        <CommandPalette
          commands={commands}
          firstGroup={activeGroup.id}
          targets={commandTargets}
          onRun={runCommand}
          onClose={() => {
            setPaletteOpen(false);
            focusActiveTerminal();
          }}
        />
      )}
      {importing && <ImportDialog onClose={closeImport} onImported={reloadProfiles} />}
      {settingsOpen && <SettingsDialog localAllowed={localAllowed} onClose={closeSettings} />}
      {closingWindow !== null && (
        <ConfirmDialog
          title={t("closeConfirm.windowTitle")}
          message={t("closeConfirm.window", { count: closingWindow })}
          confirmLabel={t("closeConfirm.windowConfirm")}
          danger
          onConfirm={confirmCloseWindow}
          onCancel={cancelCloseWindow}
        />
      )}
      {closing && (
        <ConfirmDialog
          title={
            closing.tabs === 0
              ? t("closeConfirm.paneTitle")
              : closing.tabs > 1
                ? t("closeConfirm.titleMany", { count: closing.tabs })
                : t("closeConfirm.title")
          }
          message={closeMessage(closing)}
          confirmLabel={t("closeConfirm.confirm")}
          danger
          checkboxLabel={t("common.dontAskAgain")}
          onConfirm={confirmClose}
          onCancel={cancelClose}
        />
      )}
      <Tooltips />
    </div>
  );
}

export default App;
