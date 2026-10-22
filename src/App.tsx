import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { useLocale } from "./hooks/locale";
import { useLatest } from "./hooks/latest";
import { useAppMenu } from "./app/useAppMenu";
import { useCloseFlow } from "./app/useCloseFlow";
import { useCompose } from "./app/useCompose";
import { useDirectorySync } from "./app/useDirectorySync";
import { usePaneActions } from "./app/usePaneActions";
import { PaneDragLayer, usePaneDrag } from "./app/usePaneDrag";
import { useFullScreen } from "./app/useFullScreen";
import { useQuickCommandBar } from "./app/useQuickCommandBar";
import { useStartupInfo } from "./app/useStartupInfo";
import { useKeepAwake } from "./app/useKeepAwake";
import { useTaskbarProgress } from "./app/useTaskbarProgress";
import { useTerminalMenu } from "./app/useTerminalMenu";
import { useWindows } from "./app/useWindows";

import { useAppLock } from "./hooks/appLock";
import { CommandPalette } from "./components/CommandPalette";
import { ComposeBar } from "./components/ComposeBar";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ImportDialog } from "./components/ImportDialog";
import { ProfileDialog, type ProfileDefaults } from "./components/ProfileDialog";
import { QuickCommandBar } from "./components/QuickCommandBar";
import { SettingsDialog } from "./components/SettingsDialog";
import type { SectionId } from "./components/settings/common";
import { Sidebar, type OpenMode, type SidebarHandlers } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { TabBar, type TabBarHandlers } from "./components/TabBar";
import { PaneHostList } from "./components/PaneHost";
import { createPaneHosts } from "./components/paneHosts";
import { TabPage, type PaneHandlers } from "./components/TabPage";
import { Tooltips } from "./components/Tooltip";
import i18n from "./i18n/index";
import { errorMessage, sendBreak, sessionLog, type Folder, type Profile, type QuickCommand, type SessionTarget } from "./ipc/api";
import { ISSUES_URL, RELEASES_URL, useNativeMenuLabel } from "./hooks/appMenu";
import { actionOf, matches, paneSideOf, tabIndexOf, type MenuAction } from "./lib/keymap";
import { neighbor } from "./lib/layout";
import { quickCommandStore, tabGroup } from "./lib/quickCommands";
import { asTyped } from "./lib/compose";
import { findPane, focusedPane, targetProtocol, type Pane, type SidePanel, type Tab } from "./lib/panes";
import { address, sessionsIn, type QuickTarget } from "./lib/sessions";
import { announce } from "./lib/announce";
import { directory } from "./lib/directory";
import { useDirectory, useQuickCommands, useTabStore } from "./hooks/stores";
import { createSessionRegistry } from "./lib/sessionRegistry";
import { createTabRest } from "./lib/tabRest";
import { currentSettings } from "./lib/settings";
import { useSettings } from "./hooks/settings";
import { useShortcuts } from "./hooks/shortcuts";
import { useStableHandlers } from "./hooks/stableHandlers";
import { activeTabOf, createTabStore, type PanePatch } from "./lib/tabs";
import { tabMark } from "./lib/terminalSchemes";
import { useUpdates } from "./hooks/updates";
import { checkForUpdates } from "./lib/updates";
import "./styles.css";

/** What terminal full screen hides, whose keys and menu items do nothing meanwhile. */
const HIDDEN_IN_FULL_SCREEN = ["searchSessions", "composeBar", "filePanel", "forwardsPanel"] as const;

function App() {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const { settings, update, saveError: settingsSaveError, dismissSaveError: dismissSettingsSaveError } = useSettings();
  useDirectorySync();
  const { profiles } = useDirectory();
  const [searchFocusKey, setSearchFocusKey] = useState(0);
  // A folder's sessions waiting for confirmation to open them all.
  const [openingAll, setOpeningAll] = useState<{ folder: Folder; profiles: Profile[] } | null>(null);
  const [store] = useState(() => createTabStore());
  const { tabs, activeKey } = useTabStore(store);
  const { dispatch } = store;
  const activate = (key: number | null) => dispatch({ type: "activate", key });
  // The profile dialog: an existing profile, or a new one (null) with defaults; `paneKey` is
  // the quick connection pane being saved as a session.
  const [editing, setEditing] = useState<{ profile: Profile | null; defaults?: ProfileDefaults; paneKey?: number } | null>(null);
  const [importing, setImporting] = useState(false);
  // Open, scrolled to a section if one is named.
  // A new object each time they are asked for, so that a section asked for while they are
  // open is shown too.
  const [settingsOpen, setSettingsOpen] = useState<false | { section?: SectionId }>(false);
  const openSettings = useCallback((section?: SectionId) => setSettingsOpen({ section }), []);
  const { shellName, localAllowed, username, setAsideDialog } = useStartupInfo();
  // What closes panes and moves them between windows, made further down, hears the registry
  // through `sessions.on`.
  const [sessions] = useState(() =>
    createSessionRegistry(store, {
      profiles: () => directory.get().profiles,
      t: i18n.t,
      announce,
      markOutput: () => currentSettings().tabs.markOutput,
      bell: () => currentSettings().terminal,
      notifications: () => currentSettings().terminal.notifications,
    }),
  );
  const localTitleRef = useLatest(shellName ?? t("tabs.localTitle"));

  /** Moves the keyboard focus back to the active terminal (after a bar or the palette closes). */
  const focusActiveTerminal = useCallback(
    () =>
      requestAnimationFrame(() => {
        const tab = activeTabOf(store.get());
        if (tab) sessions.focus(tab.focused);
      }),
    [store, sessions],
  );
  useTaskbarProgress(tabs);
  useKeepAwake(tabs);
  const quick = useQuickCommandBar();
  const { commands, saveError: quickSaveError } = useQuickCommands();
  const fullScreen = useFullScreen(tabs.length > 0, (on) => {
    announce(on ? t("announce.fullScreen", { shortcut: shortcutLabel("fullScreen") }) : t("announce.fullScreenLeft"));
    focusActiveTerminal();
  });
  useNativeMenuLabel("fullScreen", t(fullScreen.on ? "appMenu.exitFullScreen" : "appMenu.items.fullScreen"));
  const composer = useCompose(store, sessions, tabs, activeKey, {
    followRemoteTitle: settings.tabs.followRemoteTitle,
    focusTerminal: focusActiveTerminal,
  });
  const { compose, flashing, inScope } = composer;
  const close = useCloseFlow(store, sessions, settings, update);
  const { requestClose, closeFocused } = close;

  /** What a tab's next session will be: a saved session's protocol may have been changed. */
  const protocolOf = useCallback((target: SessionTarget) => targetProtocol(target, directory.get().profiles), []);
  const paneActions = usePaneActions(store, sessions, protocolOf);
  const { refused, onAreaSize, roomToSplit, splitPane, duplicateTab } = paneActions;

  const addTab = useCallback(
    (target: SessionTarget, title: string) => dispatch({ type: "open", pane: { target, protocol: protocolOf(target), title } }),
    [dispatch, protocolOf],
  );

  const windowing = useWindows(store, sessions, { followRemoteTitle: settings.tabs.followRemoteTitle, openTab: addTab });
  const [tabRest] = useState(() =>
    createTabRest(
      () => store.get().activeKey,
      (key) => store.dispatch({ type: "activate", key }),
    ),
  );
  const paneDrag = usePaneDrag(store, paneActions, windowing, tabRest);

  /** Shows the pane: activates its tab and focuses it there. */
  const focusPane = useCallback((key: number) => dispatch({ type: "focusPane", key }), [dispatch]);

  /**
   * "connect" connects again in the focused pane of the active tab when that is the session's
   * and its connection is closed (or waiting to reconnect), else opens a tab; "newTab" always
   * opens a tab; "splitRight" and "splitDown" split the focused pane (or open a tab if there
   * is none); "newWindow" opens a window with a tab for it.
   */
  const openProfile = (profile: Profile, mode: OpenMode) => {
    directory.opened(profile.id);
    if (mode === "newWindow") {
      windowing.openInNewWindow(profile.id, profile.name);
      return;
    }
    const target: SessionTarget = { kind: "profile", profileId: profile.id };
    const active = activeTabOf(store.get());
    const here = active?.panes.find((p) => p.key === active.focused);
    if (mode === "connect" && here?.status === "closed" && here.target.kind === "profile" && here.target.profileId === profile.id) {
      sessions.reconnect(here.key);
      focusActiveTerminal();
    } else if ((mode === "splitRight" || mode === "splitDown") && active) {
      splitPane(active.focused, mode === "splitRight" ? "row" : "column", {
        target,
        protocol: protocolOf(target),
        title: profile.name,
      });
    } else addTab(target, profile.name);
  };

  const openAll = (folder: Folder) => {
    const list = sessionsIn(folder.id, directory.get());
    // Several tabs (and connections) at once ask first; a single session opens as it would by itself.
    if (list.length > 1) setOpeningAll({ folder, profiles: list });
    else list.forEach((p) => openProfile(p, "newTab"));
  };

  // SSH without a user name logs in as the local user.
  const quickConnect = (target: QuickTarget) =>
    addTab({ kind: "quick", ...target }, address({ ...target, username: target.username || (target.protocol === "ssh" ? username : "") }));

  const openLocalTab = useCallback(() => addTab({ kind: "local" }, localTitleRef.current), [addTab, localTitleRef]);

  const moveTab = (key: number, index: number) => dispatch({ type: "move", key, index });

  /** Runs `f` on the focused pane of the tab with this key (tab menu actions). */
  const withFocused = (key: number, f: (pane: Pane) => unknown) => {
    const pane = focusedOf(key);
    if (pane) f(pane);
  };

  const updateTab = (key: number, patch: Partial<Tab>) => dispatch({ type: "updateTab", key, patch });

  /** Changes a pane, wherever it is. */
  const updatePane = (key: number, patch: PanePatch) => dispatch({ type: "updatePane", key, patch });

  /** The focused pane of a tab, by the tab's key. */
  const focusedOf = (key: number) => {
    const tab = store.get().tabs.find((t) => t.key === key);
    return tab && focusedPane(tab);
  };

  // Opens a panel only for an SSH pane; closes it whatever the pane.
  const togglePanel = useCallback((panel: SidePanel) => dispatch({ type: "togglePanel", panel }), [dispatch]);

  // The "Lock" action: the app lock's settings when it is off, to turn it on.
  const { lock } = useAppLock();
  const lockApp = useCallback(() => !lock() && openSettings("security"), [lock, openSettings]);

  // Settings open over a dialog too, and the app locks over one.
  useShortcuts(
    (e) => {
      if (matches(e, "lock")) lockApp();
      else if (matches(e, "settings")) openSettings();
      else if (matches(e, "about")) openSettings("about");
      else if (matches(e, "checkForUpdates")) {
        // The result shows there.
        openSettings("about");
        checkForUpdates();
      } else if (matches(e, "releaseNotes")) void openUrl(RELEASES_URL).catch(console.error);
      else if (matches(e, "reportIssue")) void openUrl(ISSUES_URL).catch(console.error);
      else return false;
      return true;
    },
    { when: "always" },
  );

  useShortcuts((e) => {
    if (fullScreen.on && actionOf(e, HIDDEN_IN_FULL_SCREEN)) return true;
    if (matches(e, "newLocalTerminal") && localAllowed) {
      openLocalTab();
      return true;
    }
    if (matches(e, "searchSessions")) {
      setSearchFocusKey((key) => key + 1);
      return true;
    }
    if (matches(e, "importSshConfig")) {
      setImporting(true);
      return true;
    }
    // The focused pane or tab, or the window once it has no tabs.
    if (matches(e, "closeTab")) {
      closeFocused();
      return true;
    }
    const tabAction = actionOf(e, ["nextTab", "previousTab"]);
    const tabIndex = tabIndexOf(e);
    if (tabAction || tabIndex !== null) {
      const tabs = store.get().tabs;
      const index = tabs.findIndex((t) => t.key === store.get().activeKey);
      if (tabs.length === 0) return true;
      let next: number;
      if (tabAction === "nextTab") next = (index + 1) % tabs.length;
      else if (tabAction === "previousTab") next = (index - 1 + tabs.length) % tabs.length;
      // ⌘9 / Alt+9 is always the last tab, as in browsers.
      else next = tabIndex === -1 ? tabs.length - 1 : tabIndex!;
      if (next < tabs.length) activate(tabs[next].key);
      return true;
    }
    const split = actionOf(e, ["splitRight", "splitDown"]);
    const side = paneSideOf(e);
    if ((split || side) && store.get().activeKey !== null) {
      const tab = activeTabOf(store.get());
      if (!tab) return true;
      if (split) splitPane(tab.focused, split === "splitRight" ? "row" : "column");
      else {
        const next = neighbor(tab.layout, tab.focused, side!);
        if (next !== null) focusPane(next);
      }
      return true;
    }
    if (matches(e, "fullScreen")) {
      if (store.get().tabs.length > 0) fullScreen.toggle();
      return true;
    }
    if (matches(e, "quickCommands")) {
      if (store.get().tabs.length > 0) quick.setPaletteOpen(true);
      return true;
    }
    if (matches(e, "composeBar")) {
      if (store.get().tabs.length > 0) composer.toggle();
      return true;
    }
    const panel = actionOf(e, ["filePanel", "forwardsPanel"]);
    if (!panel) return false;
    togglePanel(panel === "filePanel" ? "files" : "forwards");
    return true;
  });

  const runCommand = (command: QuickCommand) => composer.send(asTyped(command.text, command.enter));

  const activeTab = tabs.find((tab) => tab.key === activeKey);
  const activePane = activeTab && focusedPane(activeTab);
  // Looked up for every tab on each render: by id rather than through the whole list.
  const profileById = new Map(profiles.map((profile) => [profile.id, profile]));
  const profileOf = (pane: Pane) => (pane.target.kind === "profile" ? profileById.get(pane.target.profileId) : undefined);
  const groupOf = (pane: Pane) => commands && tabGroup(commands, pane.commandGroup, profileOf(pane)?.commandGroup);
  const activeGroup = activePane ? groupOf(activePane) : null;
  const inScopeKeys = inScope.map((pane) => pane.key);
  /** Tabs with any of these panes. */
  const tabsWith = (keys: number[]) => tabs.filter((t) => t.panes.some((p) => keys.includes(p.key))).map((t) => t.key);

  /** Whether an item of the app menu applies now, outside macOS (whose items are always on). */
  const menuEnabled = (action: MenuAction): boolean => {
    if (fullScreen.on && (HIDDEN_IN_FULL_SCREEN as readonly MenuAction[]).includes(action)) return false;
    // The file and port forwarding panels open only for SSH, and close for anything.
    const panel = (kind: SidePanel) => !!activeTab && (activeTab.sidePanel === kind || activePane?.protocol === "ssh");
    switch (action) {
      case "newLocalTerminal":
        return localAllowed;
      case "exportSessions":
        return profiles.length > 0;
      case "copy":
        return !!activePane && sessions.hasSelection(activePane.key);
      case "paste":
      case "find":
      case "composeBar":
      case "fullScreen":
        return !!activePane;
      case "splitRight":
      case "splitDown":
        return !!activePane && activePane.protocol !== "serial" && roomToSplit(activePane.key, action === "splitRight" ? "row" : "column");
      case "filePanel":
        return panel("files");
      case "forwardsPanel":
        return panel("forwards");
      case "quickCommands":
        return !!activePane && !!commands && !!activeGroup;
      default:
        return true;
    }
  };
  const updates = useUpdates();
  const appMenu = useAppMenu({ enabled: menuEnabled, fullScreen: fullScreen.on, updates });

  // A newer version found while the app runs (not one known at the start, marked already),
  // told in the window in use; Settings › About tells of the checks made there.
  const announcedUpdate = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    if (!updates.loaded) return;
    const version = updates.available?.version ?? null;
    const known = announcedUpdate.current;
    announcedUpdate.current = version;
    if (known !== undefined && version && version !== known && document.hasFocus() && !settingsOpen) {
      announce(t("updates.found", { version }));
    }
  }, [updates.loaded, updates.available?.version, settingsOpen, t]);

  const terminalMenu = useTerminalMenu({
    tabs,
    paneActions,
    windowing,
    requestClose,
    fullScreen,
    followRemoteTitle: settings.tabs.followRemoteTitle,
    commandsOf: (pane) => {
      const group = groupOf(pane);
      return !commands || !group || commands.groups.every((g) => g.commands.length === 0) ? null : group.commands;
    },
    runCommand,
    openPalette: () => quick.setPaletteOpen(true),
  });

  /** Starts or stops logging a pane's session. */
  const setLogging = (key: number, start: boolean) => {
    const pane = findPane(store.get().tabs, key)?.pane;
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
    const path = findPane(store.get().tabs, key)?.pane.logPath;
    if (path) revealItemInDir(path).catch(console.error);
  };

  const sidebarHandlers = useStableHandlers<SidebarHandlers>({
    onOpen: openProfile,
    onOpenAll: openAll,
    onQuickConnect: quickConnect,
    onEdit: (profile) => setEditing({ profile }),
    onNew: (folder) => setEditing({ profile: null, defaults: { folder } }),
    onImportSshConfig: () => setImporting(true),
    onSettings: () => openSettings(),
  });

  const tabBarHandlers = useStableHandlers<TabBarHandlers>({
    onSelect: activate,
    onNew: openLocalTab,
    onClose: (keys) => void requestClose({ kind: "tabs", keys }),
    onSplit: (key, direction) => withFocused(key, (pane) => splitPane(pane.key, direction)),
    canSplit: (key, direction) => {
      const pane = focusedOf(key);
      return !!pane && roomToSplit(pane.key, direction);
    },
    onMove: moveTab,
    canMoveToWindow: (key) => {
      const tab = store.get().tabs.find((t) => t.key === key);
      return !!tab && windowing.canMove(tab);
    },
    onMoveToWindow: (key, target) => void windowing.moveTab(key, target),
    onDropOut: windowing.dropTab,
    onOverPanes: paneDrag.overPanes,
    onDropOnPanes: paneDrag.dropOnPanes,
    onRename: (key, customTitle) => updateTab(key, { customTitle }),
    onDuplicate: duplicateTab,
    onSaveAsSession: (key) => withFocused(key, (pane) => saveAsSession(pane.key)),
    onReconnect: (key) => withFocused(key, (pane) => sessions.reconnect(pane.key)),
    onBreak: (key) => withFocused(key, (pane) => pane.sessionId != null && sendBreak(pane.sessionId).catch(console.error)),
    onLog: (key, start) => withFocused(key, (pane) => setLogging(pane.key, start)),
    onShowLog: (key) => withFocused(key, (pane) => showLog(pane.key)),
    onTogglePanel: togglePanel,
    onToggleCompose: composer.toggle,
    onToggleQuickBar: quick.toggleBar,
  });

  const [hosts] = useState(createPaneHosts);
  const handlers = useStableHandlers<PaneHandlers>({
    sessions,
    hosts,
    onTitle: (key, title) => updatePane(key, { remoteTitle: title || null }),
    onInput: composer.onInput,
    pasteTargets: composer.syncedPasteTargets,
    onFocus: (key) => {
      const found = findPane(store.get().tabs, key);
      if (found && found.tab.focused !== key) updateTab(found.tab.key, { focused: key });
    },
    onLayout: (tabKey, layout) => updateTab(tabKey, { layout }),
    onAreaSize,
    onTransfers: (key, count) => updatePane(key, { transfers: count }),
    onEdits: (key, count) => updatePane(key, { edits: count }),
    onQuestions: (key, count) => updatePane(key, { questions: count }),
    menuItems: terminalMenu,
    onGrab: paneDrag.start,
  });

  const closeDialog = useCallback(() => setEditing(null), []);

  // A quick connection saved as a session becomes that session's pane; the connection stays.
  const onProfileSaved = (profile: Profile) => {
    const paneKey = editing?.paneKey;
    if (paneKey === undefined) return;
    updatePane(paneKey, { target: { kind: "profile", profileId: profile.id }, title: profile.name });
  };

  const saveAsSession = (key: number) => {
    const pane = findPane(store.get().tabs, key)?.pane;
    if (pane?.target.kind !== "quick") return;
    const { protocol, host, port } = pane.target;
    const user = pane.target.username || (protocol === "ssh" ? username : "");
    setEditing({ profile: null, defaults: { protocol, host, port, username: user }, paneKey: key });
  };
  const closeImport = useCallback(() => setImporting(false), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);

  return (
    <div className={`app${fullScreen.on ? " full-screen" : ""}`}>
      <Sidebar
        searchFocusKey={searchFocusKey}
        handlers={sidebarHandlers}
        appMenu={appMenu.button}
        appMenuOpen={appMenu.open}
        update={updates.available}
      />
      <main>
        <TabBar
          tabs={tabs}
          activeKey={activeKey}
          followRemoteTitle={settings.tabs.followRemoteTitle}
          markOutput={settings.tabs.markOutput}
          bellAlert={settings.terminal.bellAlert}
          handlers={tabBarHandlers}
          tabRest={tabRest}
          canOpenLocal={localAllowed}
          windows={windowing.others}
          composeOpen={compose.open}
          quickBarOpen={quick.barOpen}
          inScope={tabsWith(inScopeKeys)}
          syncing={compose.open && compose.sync}
          flashing={tabsWith(flashing)}
          addressOf={(tab) => {
            const pane = focusedPane(tab);
            if (pane.target.kind === "local") return undefined;
            const profile = profileOf(pane);
            return pane.target.kind === "quick" ? address(pane.target) : profile && address(profile.connection);
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
            onChange={composer.setCompose}
            onSend={(text) => composer.send(asTyped(text, true))}
            onClose={composer.toggle}
          />
        )}
        <div className="terminals">
          {tabs.map((tab) => (
            <TabPage
              key={tab.key}
              tab={tab}
              active={tab.key === activeKey}
              syncing={compose.open && compose.sync && tab.key === activeKey ? tab.focused : null}
              inScope={inScopeKeys}
              flashing={flashing}
              refused={refused}
              draggable={tabs.length > 1 || tab.panes.length > 1 || windowing.others.length > 0}
              handlers={handlers}
            />
          ))}
          <PaneHostList tabs={tabs} activeKey={activeKey} profiles={profiles} handlers={handlers} />
          {fullScreen.hint && (
            // Announced as well, when it starts.
            <div className="full-screen-hint" aria-hidden="true">
              {t("app.fullScreenHint", { shortcut: shortcutLabel("fullScreen") })}
            </div>
          )}
          {tabs.length === 0 && <div className="placeholder">{t(localAllowed ? "app.placeholder" : "app.placeholderNoLocal")}</div>}
        </div>
        {quick.barOpen && activePane && commands && activeGroup && (
          <QuickCommandBar
            group={activeGroup}
            onPickGroup={(id) => updatePane(activePane.key, { commandGroup: id })}
            onRun={runCommand}
            targets={composer.targets}
          />
        )}
        {settings.statusBar.show && activePane && <StatusBar pane={activePane} profile={profileOf(activePane)} sessions={sessions} />}
      </main>
      {editing && <ProfileDialog profile={editing.profile} defaults={editing.defaults} onClose={closeDialog} onSaved={onProfileSaved} />}
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
      {quick.paletteOpen && commands && activeGroup && (
        <CommandPalette
          firstGroup={activeGroup.id}
          targets={composer.targets}
          onRun={runCommand}
          onClose={() => {
            quick.setPaletteOpen(false);
            focusActiveTerminal();
          }}
        />
      )}
      {importing && <ImportDialog onClose={closeImport} />}
      {settingsOpen && <SettingsDialog localAllowed={localAllowed} request={settingsOpen} onClose={closeSettings} />}
      {close.dialogs}
      {appMenu.menu}
      {/* A change that couldn't be saved was undone, maybe after its dialog closed. */}
      {settingsSaveError !== null && (
        <ConfirmDialog
          notice
          title={t("saveFailed.title")}
          message={t("saveFailed.settings", { error: errorMessage(settingsSaveError) })}
          confirmLabel={t("common.ok")}
          onConfirm={dismissSettingsSaveError}
          onCancel={dismissSettingsSaveError}
        />
      )}
      {quickSaveError !== null && (
        <ConfirmDialog
          notice
          title={t("saveFailed.title")}
          message={t("saveFailed.quickCommands", { error: errorMessage(quickSaveError) })}
          confirmLabel={t("common.ok")}
          onConfirm={quickCommandStore.dismissSaveError}
          onCancel={quickCommandStore.dismissSaveError}
        />
      )}
      {setAsideDialog}
      <PaneDragLayer view={paneDrag.view} />
      <Tooltips />
    </div>
  );
}

export default App;
