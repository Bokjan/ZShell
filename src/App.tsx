import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { useCloseFlow } from "./app/useCloseFlow";
import { useCompose } from "./app/useCompose";
import { usePaneActions } from "./app/usePaneActions";
import { useQuickCommands } from "./app/useQuickCommands";

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
import { TabPage, type PaneHandlers } from "./components/TabPage";
import { Tooltips } from "./components/Tooltip";
import i18n from "./i18n";
import {
  configSetAside,
  listProfiles,
  localShellName,
  localUsername,
  sendBreak,
  sessionLog,
  tree,
  type Folder,
  type Profile,
  type QuickCommand,
  type SessionTarget,
  type SetAsideFile,
} from "./lib/api";
import { basename } from "./lib/format";
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
import { neighbor } from "./lib/layout";
import { tabGroup } from "./lib/quickCommands";
import { asTyped } from "./lib/compose";
import { findPane, focusedPane, targetProtocol, type Pane, type SidePanel, type Tab } from "./lib/panes";
import { addRecent, address, sessionsIn, storedRecent, type QuickTarget } from "./lib/sessions";
import { announce } from "./lib/announce";
import { createSessionRegistry } from "./lib/sessionRegistry";
import { useSettings } from "./lib/settings";
import { useShortcuts } from "./lib/shortcuts";
import { useStableHandlers } from "./lib/stableHandlers";
import { activeTabOf, createTabStore, useTabStore, type PanePatch } from "./lib/tabs";
import { tabMark } from "./lib/terminalSchemes";
import { useTitleBar } from "./lib/window";
import "./styles.css";

const profileIdOf = (pane: Pane) => (pane.target.kind === "profile" ? pane.target.profileId : undefined);

/** Opening more sessions than this at once (a folder) asks first. */
const OPEN_ALL_CONFIRM = 5;
/** Quick commands listed in the terminal's menu; the palette has them all. */
const MENU_COMMANDS = 8;

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
  const [setAside, setSetAside] = useState<SetAsideFile[]>([]);
  const [store] = useState(() => createTabStore());
  const { tabs, activeKey } = useTabStore(store);
  const { dispatch } = store;
  const activate = (key: number | null) => dispatch({ type: "activate", key });
  // The profile dialog: an existing profile, or a new one (null) with defaults; `paneKey` is
  // the quick connection pane being saved as a session.
  const [editing, setEditing] = useState<{ profile: Profile | null; defaults?: ProfileDefaults; paneKey?: number } | null>(
    null,
  );
  const [importing, setImporting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Title for local terminal tabs, e.g. "zsh".
  const [shellName, setShellName] = useState<string | null>(null);
  // False on Windows in S mode, which blocks local shells.
  const [localAllowed, setLocalAllowed] = useState(true);
  // For quick connections without a user name.
  const [username, setUsername] = useState("");
  const profilesRef = useRef(profiles);
  profilesRef.current = profiles;
  const [sessions] = useState(() =>
    createSessionRegistry(store, { profiles: () => profilesRef.current, t: i18n.t, announce }),
  );
  const localTitleRef = useRef("");
  localTitleRef.current = shellName ?? t("tabs.localTitle");

  /** Moves the keyboard focus back to the active terminal (after a bar or the palette closes). */
  const focusActiveTerminal = useCallback(
    () =>
      requestAnimationFrame(() => {
        const tab = activeTabOf(store.get());
        if (tab) sessions.focus(tab.focused);
      }),
    [store, sessions],
  );
  const quick = useQuickCommands();
  const { commands } = quick;
  const composer = useCompose(store, sessions, tabs, activeKey, {
    followRemoteTitle: () => settingsRef.current.tabs.followRemoteTitle,
    focusTerminal: focusActiveTerminal,
  });
  const { compose, flashing, inScope } = composer;
  const close = useCloseFlow(store, settings, update);
  const { requestClose, closeFocused } = close;

  const reloadProfiles = useCallback(() => {
    listProfiles().then(setProfiles).catch(console.error);
    tree.folders().then(setFolders).catch(console.error);
  }, []);
  useEffect(reloadProfiles, [reloadProfiles]);
  // Returned only once, so a second call (StrictMode) must not clear the first's result.
  useEffect(() => {
    configSetAside()
      .then((files) => files.length > 0 && setSetAside(files))
      .catch(console.error);
  }, []);

  useEffect(() => {
    localShellName()
      .then((name) => (name === null ? setLocalAllowed(false) : setShellName(name)))
      .catch(console.error);
    localUsername().then(setUsername).catch(console.error);
  }, []);

  /** What a tab's next session will be: a saved session's protocol may have been changed. */
  const protocolOf = useCallback((target: SessionTarget) => targetProtocol(target, profilesRef.current), []);
  const { refused, onAreaSize, roomToSplit, splitPane, duplicateTab } = usePaneActions(store, protocolOf);

  const addTab = useCallback(
    (target: SessionTarget, title: string) => dispatch({ type: "open", pane: { target, protocol: protocolOf(target), title } }),
    [dispatch, protocolOf],
  );

  /** Shows the pane: activates its tab and focuses it there. */
  const focusPane = useCallback((key: number) => dispatch({ type: "focusPane", key }), [dispatch]);

  /**
   * "connect" switches to a pane the session already has; "newTab" always opens a tab;
   * "splitRight" and "splitDown" split the focused pane (or open a tab if there is none).
   */
  const openProfile = (profile: Profile, mode: OpenMode) => {
    setRecent(addRecent(profile.id));
    const target: SessionTarget = { kind: "profile", profileId: profile.id };
    const existing = store.get().tabs
      .flatMap((t) => t.panes)
      .find((p) => p.target.kind === "profile" && p.target.profileId === profile.id);
    const active = activeTabOf(store.get());
    if (mode === "connect" && existing) focusPane(existing.key);
    else if ((mode === "splitRight" || mode === "splitDown") && active) {
      splitPane(active.focused, mode === "splitRight" ? "row" : "column", {
        target,
        protocol: protocolOf(target),
        title: profile.name,
      });
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

  // Settings open over a dialog too.
  useShortcuts(
    (e) => {
      if (!isSettingsShortcut(e)) return false;
      setSettingsOpen(true);
      return true;
    },
    { when: "always" },
  );

  useShortcuts((e) => {
    if (isNewTabShortcut(e) && localAllowed) {
      openLocalTab();
      return true;
    }
    if (isSearchShortcut(e)) {
      setSearchFocusKey((key) => key + 1);
      return true;
    }
    const tabKey = tabShortcut(e);
    if (tabKey) {
      const tabs = store.get().tabs;
      const index = tabs.findIndex((t) => t.key === store.get().activeKey);
      if (tabs.length === 0) return true;
      if (tabKey.type === "close") {
        closeFocused();
        return true;
      }
      let next: number;
      if (tabKey.type === "next") next = (index + 1) % tabs.length;
      else if (tabKey.type === "previous") next = (index - 1 + tabs.length) % tabs.length;
      // ⌘9 / Alt+9 is always the last tab, as in browsers.
      else next = tabKey.index === 8 ? tabs.length - 1 : tabKey.index;
      if (next < tabs.length) activate(tabs[next].key);
      return true;
    }
    const split = splitShortcut(e);
    const side = paneFocusShortcut(e);
    if ((split || side) && store.get().activeKey !== null) {
      const tab = activeTabOf(store.get());
      if (!tab) return true;
      if (split) splitPane(tab.focused, split);
      else {
        const next = neighbor(tab.layout, tab.focused, side!);
        if (next !== null) focusPane(next);
      }
      return true;
    }
    if (e.code === "KeyJ" && hasShiftShortcutModifiers(e)) {
      if (store.get().tabs.length > 0) quick.setPaletteOpen(true);
      return true;
    }
    if (e.code === "KeyI" && hasShiftShortcutModifiers(e)) {
      if (store.get().tabs.length > 0) composer.toggle();
      return true;
    }
    const panel = PANEL_SHORTCUTS[e.code];
    if (!panel || !hasShiftShortcutModifiers(e)) return false;
    togglePanel(panel);
    return true;
  });

  // From the macOS app menu's "Settings…" item (⌘,).
  useEffect(() => {
    const unlisten = listen("open-settings", () => setSettingsOpen(true));
    return () => void unlisten.then((f) => f());
  }, []);

  const runCommand = (command: QuickCommand) => composer.send(asTyped(command.text, command.enter));

  const activeTab = tabs.find((tab) => tab.key === activeKey);
  const activePane = activeTab && focusedPane(activeTab);
  const profileOf = (pane: Pane) => profiles.find((p) => p.id === profileIdOf(pane));
  const groupOf = (pane: Pane) => commands && tabGroup(commands, pane.commandGroup, profileOf(pane)?.commandGroup);
  const activeGroup = activePane ? groupOf(activePane) : null;
  const inScopeKeys = inScope.map((pane) => pane.key);
  /** Tabs with any of these panes. */
  const tabsWith = (keys: number[]) => tabs.filter((t) => t.panes.some((p) => keys.includes(p.key))).map((t) => t.key);

  /** Splitting and closing the pane, then quick commands: those of the pane's group, and the palette. */
  const terminalMenu = (pane: Pane): MenuItem[] => [...paneMenu(pane), ...commandMenu(pane)];

  const paneMenu = (pane: Pane): MenuItem[] => {
    const split = (tabs.find((tab) => tab.panes.some((p) => p.key === pane.key))?.panes.length ?? 0) > 1;
    // A serial device can only be open once.
    const serial = pane.protocol === "serial";
    return [
      "separator",
      {
        label: t("tabs.splitRight"),
        shortcut: splitRightShortcutLabel,
        disabled: serial || !roomToSplit(pane.key, "row"),
        onSelect: () => splitPane(pane.key, "row"),
      },
      {
        label: t("tabs.splitDown"),
        shortcut: splitDownShortcutLabel,
        disabled: serial || !roomToSplit(pane.key, "column"),
        onSelect: () => splitPane(pane.key, "column"),
      },
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
      { label: t("quick.paletteMenu"), shortcut: shiftShortcutLabel("J"), onSelect: () => quick.setPaletteOpen(true) },
    ];
  };

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

  const onProfileChanged = (updated: Profile) =>
    setProfiles((profiles) => profiles.map((p) => (p.id === updated.id ? updated : p)));

  const handlers = useStableHandlers<PaneHandlers>({
    sessions,
    onTitle: (key, title) => updatePane(key, { remoteTitle: title || null }),
    onInput: composer.onInput,
    registerPaste: composer.registerPaste,
    pasteTargets: composer.syncedPasteTargets,
    onFocus: (key) => {
      const found = findPane(store.get().tabs, key);
      if (found && found.tab.focused !== key) updateTab(found.tab.key, { focused: key });
    },
    onLayout: (tabKey, layout) => updateTab(tabKey, { layout }),
    onAreaSize,
    onTransfers: (key, count) => updatePane(key, { transfers: count }),
    menuItems: terminalMenu,
    profileOf,
    onProfileChanged,
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
          onSelect={activate}
          onNew={localAllowed ? openLocalTab : undefined}
          onClose={(keys) => void requestClose({ kind: "tabs", keys })}
          onSplit={(key, direction) => withFocused(key, (pane) => splitPane(pane.key, direction))}
          canSplit={(key, direction) => {
            const pane = focusedOf(key);
            return !!pane && roomToSplit(pane.key, direction);
          }}
          onMove={moveTab}
          onRename={(key, customTitle) => updateTab(key, { customTitle })}
          onDuplicate={duplicateTab}
          onSaveAsSession={(key) => withFocused(key, (pane) => saveAsSession(pane.key))}
          onReconnect={(key) => withFocused(key, (pane) => sessions.reconnect(pane.key))}
          onBreak={(key) =>
            withFocused(key, (pane) => pane.sessionId != null && sendBreak(pane.sessionId).catch(console.error))
          }
          onLog={(key, start) => withFocused(key, (pane) => setLogging(pane.key, start))}
          onShowLog={(key) => withFocused(key, (pane) => showLog(pane.key))}
          onTogglePanel={togglePanel}
          composeOpen={compose.open}
          onToggleCompose={composer.toggle}
          quickBarOpen={quick.barOpen}
          onToggleQuickBar={quick.toggleBar}
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
              profiles={profiles}
              handlers={handlers}
            />
          ))}
          {tabs.length === 0 && <div className="placeholder">{t(localAllowed ? "app.placeholder" : "app.placeholderNoLocal")}</div>}
        </div>
        {quick.barOpen && activePane && commands && activeGroup && (
          <QuickCommandBar
            commands={commands}
            group={activeGroup}
            onPickGroup={(id) => updatePane(activePane.key, { commandGroup: id })}
            onChange={quick.save}
            onRun={runCommand}
            targets={composer.targets}
          />
        )}
      </main>
      {editing && (
        <ProfileDialog
          profile={editing.profile}
          defaults={editing.defaults}
          profiles={profiles}
          commandGroups={commands?.groups ?? null}
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
      {quick.paletteOpen && commands && activeGroup && (
        <CommandPalette
          commands={commands}
          firstGroup={activeGroup.id}
          targets={composer.targets}
          onRun={runCommand}
          onClose={() => {
            quick.setPaletteOpen(false);
            focusActiveTerminal();
          }}
        />
      )}
      {importing && <ImportDialog onClose={closeImport} onImported={reloadProfiles} />}
      {settingsOpen && <SettingsDialog localAllowed={localAllowed} onClose={closeSettings} />}
      {close.dialogs}
      {setAside.length > 0 && (
        <ConfirmDialog
          title={t("setAside.title")}
          message={t("setAside.message")}
          confirmLabel={t("setAside.show")}
          cancelLabel={t("common.close")}
          onConfirm={() => {
            revealItemInDir(setAside[0].movedTo ?? setAside[0].path).catch(console.error);
            setSetAside([]);
          }}
          onCancel={() => setSetAside([])}
        >
          <ul className="set-aside-list">
            {setAside.map((file) => (
              <li key={file.path}>
                {file.movedTo
                  ? t("setAside.moved", { name: basename(file.path), movedTo: basename(file.movedTo) })
                  : t("setAside.notMoved", { name: basename(file.path) })}
                <span className="hint">{file.error}</span>
              </li>
            ))}
          </ul>
        </ConfirmDialog>
      )}
      <Tooltips />
    </div>
  );
}

export default App;
