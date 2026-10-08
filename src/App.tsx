import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "./components/ConfirmDialog";
import { ImportDialog } from "./components/ImportDialog";
import { ProfileDialog, type ProfileDefaults } from "./components/ProfileDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { SessionPane } from "./components/SessionPane";
import { PANEL_SHORTCUTS, TabBar, tabTitle, type SidePanel, type Tab } from "./components/TabBar";
import type { SessionStatus } from "./components/TerminalView";
import {
  listProfiles,
  localShellName,
  localUsername,
  sessionForeground,
  tree,
  type Folder,
  type ForwardState,
  type Profile,
  type SessionId,
  type SessionTarget,
} from "./lib/api";
import {
  hasShiftShortcutModifiers,
  isNewTabShortcut,
  isSearchShortcut,
  isSettingsShortcut,
  tabShortcut,
} from "./lib/platform";
import { addRecent, address, sessionsIn, storedRecent, type QuickTarget } from "./lib/sessions";
import { useSettings } from "./lib/settings";
import { useTitleBar } from "./lib/window";
import "./styles.css";

const profileIdOf = (tab: Tab) => (tab.target.kind === "ssh" ? tab.target.profileId : undefined);

/** Why closing a tab needs confirmation: an SSH session is connected, or a local program runs. */
type Busy = { kind: "connected" } | { kind: "process"; name: string };

/** Opening more sessions than this at once (a folder) asks first. */
const OPEN_ALL_CONFIRM = 5;

async function busyReason(tab: Tab): Promise<Busy | null> {
  if (tab.status !== "connected" || tab.sessionId == null) return null;
  if (tab.target.kind !== "local") return { kind: "connected" };
  const name = await sessionForeground(tab.sessionId).catch(() => null);
  return name === null ? null : { kind: "process", name };
}

const newTab = (key: number, target: SessionTarget, title: string, shareFrom?: SessionId): Tab => ({
  key,
  target,
  title,
  customTitle: null,
  remoteTitle: null,
  status: "connecting",
  sessionId: null,
  shareFrom,
  reconnectKey: 0,
  sidePanel: null,
  forwards: {},
});

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
  // The profile dialog: an existing profile, or a new one (null) with defaults; `tabKey` is
  // the quick connection tab being saved as a session.
  const [editing, setEditing] = useState<{ profile: Profile | null; defaults?: ProfileDefaults; tabKey?: number } | null>(
    null,
  );
  const [importing, setImporting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Tabs waiting for the user to confirm closing them, with why the first busy one is busy.
  const [closing, setClosing] = useState<{ keys: number[]; busy: Busy; tab: Tab } | null>(null);
  // The window waiting for the user to confirm closing it (quitting), with how many tabs are busy.
  const [closingWindow, setClosingWindow] = useState<number | null>(null);
  // Title for local terminal tabs, e.g. "zsh".
  const [shellName, setShellName] = useState<string | null>(null);
  // For quick connections without a user name.
  const [username, setUsername] = useState("");
  const nextKey = useRef(1);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
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
    localShellName().then(setShellName).catch(console.error);
    localUsername().then(setUsername).catch(console.error);
  }, []);

  const addTab = useCallback((target: SessionTarget, title: string) => {
    const key = nextKey.current++;
    setTabs((tabs) => [...tabs, newTab(key, target, title)]);
    setActiveKey(key);
  }, []);

  /** "connect" switches to a tab the session already has; "newTab" always opens one. */
  const openProfile = (profile: Profile, mode: "connect" | "newTab") => {
    setRecent(addRecent(profile.id));
    const existing = tabsRef.current.find((t) => t.target.kind === "ssh" && t.target.profileId === profile.id);
    if (mode === "connect" && existing) setActiveKey(existing.key);
    else addTab({ kind: "ssh", profileId: profile.id }, profile.name);
  };

  const openAll = (folder: Folder) => {
    const list = sessionsIn(folder.id, folders, profiles);
    if (list.length > OPEN_ALL_CONFIRM) setOpeningAll({ folder, profiles: list });
    else list.forEach((p) => openProfile(p, "newTab"));
  };

  const quickConnect = (target: QuickTarget) =>
    addTab({ kind: "quick", ...target }, address({ ...target, username: target.username || username }));

  const openLocalTab = useCallback(() => addTab({ kind: "local" }, localTitleRef.current), [addTab]);

  // An SSH tab whose shell is up opens its copy on the same connection; anything else opens
  // the same target anew.
  const duplicateTab = (key: number) => {
    const tabs = tabsRef.current;
    const index = tabs.findIndex((t) => t.key === key);
    if (index < 0) return;
    const tab = tabs[index];
    const shareFrom = tab.target.kind !== "local" && tab.status === "connected" ? (tab.sessionId ?? undefined) : undefined;
    const copy = newTab(nextKey.current++, tab.target, tab.title, shareFrom);
    setTabs([...tabs.slice(0, index + 1), copy, ...tabs.slice(index + 1)]);
    setActiveKey(copy.key);
  };

  const closeTabs = useCallback((keys: number[]) => {
    const tabs = tabsRef.current;
    const remaining = tabs.filter((t) => !keys.includes(t.key));
    setTabs(remaining);
    const active = tabs.findIndex((t) => t.key === activeKeyRef.current);
    if (active >= 0 && keys.includes(tabs[active].key)) {
      // The next remaining tab to the right, else the nearest one to the left.
      const next = tabs.slice(active).find((t) => !keys.includes(t.key)) ?? remaining[remaining.length - 1];
      setActiveKey(next?.key ?? null);
    }
  }, []);

  /** Closes the tabs, first asking if any is connected or running a program (see the settings). */
  const requestClose = useCallback(
    async (keys: number[]) => {
      const tabs = tabsRef.current.filter((t) => keys.includes(t.key));
      if (settingsRef.current.tabs.confirmClose) {
        const reasons = await Promise.all(tabs.map(busyReason));
        const index = reasons.findIndex((reason) => reason !== null);
        if (index >= 0) {
          setClosing({ keys, busy: reasons[index]!, tab: tabs[index] });
          return;
        }
      }
      closeTabs(keys);
    },
    [closeTabs],
  );

  const confirmClose = (dontAskAgain: boolean) => {
    if (!closing) return;
    if (dontAskAgain) update({ ...settings, tabs: { ...settings.tabs, confirmClose: false } });
    closeTabs(closing.keys);
    setClosing(null);
  };

  const cancelClose = useCallback(() => setClosing(null), []);

  // Closing the window (the close button, ⌘Q, Alt+F4) quits, so it asks like closing the
  // tabs would.
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      if (!settingsRef.current.tabs.confirmClose) return;
      const reasons = await Promise.all(tabsRef.current.map(busyReason));
      const busy = reasons.filter((reason) => reason !== null).length;
      if (busy === 0) return;
      event.preventDefault();
      setClosingWindow(busy);
    });
    return () => void unlisten.then((f) => f());
  }, []);

  const confirmCloseWindow = (dontAskAgain: boolean) => {
    if (dontAskAgain) update({ ...settings, tabs: { ...settings.tabs, confirmClose: false } });
    void getCurrentWindow().destroy();
  };

  const cancelCloseWindow = useCallback(() => setClosingWindow(null), []);

  const moveTab = (key: number, index: number) =>
    setTabs((tabs) => {
      const tab = tabs.find((t) => t.key === key);
      if (!tab) return tabs;
      const rest = tabs.filter((t) => t.key !== key);
      return [...rest.slice(0, index), tab, ...rest.slice(index)];
    });

  const updateTab = (key: number, patch: Partial<Tab>) =>
    setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, ...patch } : t)));

  const togglePanel = useCallback((panel: SidePanel) => {
    setTabs((tabs) =>
      tabs.map((t) =>
        t.key === activeKeyRef.current && t.target.kind !== "local"
          ? { ...t, sidePanel: t.sidePanel === panel ? null : panel }
          : t,
      ),
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
      if (isNewTabShortcut(e)) {
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
          if (index >= 0) void requestClose([tabs[index].key]);
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
      const panel = PANEL_SHORTCUTS[e.code];
      if (!panel || !hasShiftShortcutModifiers(e)) return;
      // Capture phase, so the terminal never sees the keystroke.
      e.preventDefault();
      e.stopPropagation();
      togglePanel(panel);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [togglePanel, requestClose, openLocalTab]);

  // From the macOS app menu's "Settings…" item (⌘,).
  useEffect(() => {
    const unlisten = listen("open-settings", () => setSettingsOpen(true));
    return () => void unlisten.then((f) => f());
  }, []);

  // From the macOS File menu's "Close Tab" item (⌘W), which closes the window once no tabs
  // are left, as in Terminal.app.
  useEffect(() => {
    const unlisten = listen("close-tab", () => {
      const active = activeKeyRef.current;
      if (active !== null) void requestClose([active]);
      else void getCurrentWindow().close();
    });
    return () => void unlisten.then((f) => f());
  }, [requestClose]);

  const onStatus = (key: number, status: SessionStatus) => {
    // Forward events can precede "connected" (auto-start runs right after authentication),
    // so states are only reset when a connection attempt starts or ends.
    if (status === "connected") updateTab(key, { status });
    // A new shell sets its own title.
    else if (status === "connecting") updateTab(key, { status, forwards: {}, remoteTitle: null });
    else updateTab(key, { status, forwards: {} });
  };

  // A local shell that exits cleanly (`exit`, Ctrl+D) closes its tab, like Terminal.app.
  const onExited = (tab: Tab, status: number | null) => {
    if (tab.target.kind === "local" && status === 0) closeTabs([tab.key]);
  };

  const onForward = (key: number, ruleId: string, state: ForwardState) =>
    setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, forwards: { ...t.forwards, [ruleId]: state } } : t)));

  const onProfileChanged = (updated: Profile) =>
    setProfiles((profiles) => profiles.map((p) => (p.id === updated.id ? updated : p)));

  const closeMessage = ({ keys, busy, tab }: { keys: number[]; busy: Busy; tab: Tab }) => {
    if (keys.length > 1) return t("closeConfirm.many");
    const name = tabTitle(tab, settings.tabs.followRemoteTitle);
    if (busy.kind === "connected") return t("closeConfirm.connected", { name });
    return busy.name ? t("closeConfirm.process", { process: busy.name, name }) : t("closeConfirm.processUnknown", { name });
  };

  const closeDialog = useCallback(() => setEditing(null), []);

  // A quick connection saved as a session becomes that session's tab; the connection stays.
  const onProfileSaved = (profile: Profile) => {
    const tabKey = editing?.tabKey;
    if (tabKey === undefined) return;
    updateTab(tabKey, { target: { kind: "ssh", profileId: profile.id }, title: profile.name });
  };

  const saveAsSession = (key: number) => {
    const tab = tabsRef.current.find((t) => t.key === key);
    if (tab?.target.kind !== "quick") return;
    const { host, port } = tab.target;
    setEditing({ profile: null, defaults: { host, port, username: tab.target.username || username }, tabKey: key });
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
          onNew={openLocalTab}
          onClose={(keys) => void requestClose(keys)}
          onMove={moveTab}
          onRename={(key, customTitle) => updateTab(key, { customTitle })}
          onDuplicate={duplicateTab}
          onSaveAsSession={saveAsSession}
          onReconnect={(key) =>
            setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, reconnectKey: t.reconnectKey + 1 } : t)))
          }
          onTogglePanel={togglePanel}
        />
        <div className="terminals">
          {tabs.map((tab) => (
            <SessionPane
              key={tab.key}
              tab={tab}
              profile={profiles.find((p) => p.id === profileIdOf(tab))}
              active={tab.key === activeKey}
              onStatus={(status) => onStatus(tab.key, status)}
              onExited={(status) => onExited(tab, status)}
              onSession={(sessionId) => updateTab(tab.key, { sessionId })}
              onForward={(ruleId, state) => onForward(tab.key, ruleId, state)}
              onTitle={(title) => updateTab(tab.key, { remoteTitle: title || null })}
              onProfileChanged={onProfileChanged}
            />
          ))}
          {tabs.length === 0 && <div className="placeholder">{t("app.placeholder")}</div>}
        </div>
      </main>
      {editing && (
        <ProfileDialog
          profile={editing.profile}
          defaults={editing.defaults}
          profiles={profiles}
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
      {importing && <ImportDialog onClose={closeImport} onImported={reloadProfiles} />}
      {settingsOpen && <SettingsDialog onClose={closeSettings} />}
      {closingWindow !== null && (
        <ConfirmDialog
          title={t("closeConfirm.windowTitle")}
          message={t("closeConfirm.window", { count: closingWindow })}
          confirmLabel={t("closeConfirm.windowConfirm")}
          danger
          checkboxLabel={t("common.dontAskAgain")}
          onConfirm={confirmCloseWindow}
          onCancel={cancelCloseWindow}
        />
      )}
      {closing && (
        <ConfirmDialog
          title={closing.keys.length > 1 ? t("closeConfirm.titleMany", { count: closing.keys.length }) : t("closeConfirm.title")}
          message={closeMessage(closing)}
          confirmLabel={t("closeConfirm.confirm")}
          danger
          checkboxLabel={t("common.dontAskAgain")}
          onConfirm={confirmClose}
          onCancel={cancelClose}
        />
      )}
    </div>
  );
}

export default App;
