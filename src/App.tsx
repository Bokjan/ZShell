import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "./components/ConfirmDialog";
import { ImportDialog } from "./components/ImportDialog";
import { ProfileDialog } from "./components/ProfileDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { SessionPane } from "./components/SessionPane";
import { PANEL_SHORTCUTS, TabBar, tabTitle, type SidePanel, type Tab } from "./components/TabBar";
import type { SessionStatus } from "./components/TerminalView";
import {
  listProfiles,
  localShellName,
  sessionForeground,
  type ForwardState,
  type Profile,
  type SessionId,
  type SessionTarget,
} from "./lib/api";
import { hasShiftShortcutModifiers, isSettingsShortcut, tabShortcut } from "./lib/platform";
import { useSettings } from "./lib/settings";
import "./styles.css";

const profileIdOf = (tab: Tab) => (tab.target.kind === "ssh" ? tab.target.profileId : undefined);

/** Why closing a tab needs confirmation: an SSH session is connected, or a local program runs. */
type Busy = { kind: "connected" } | { kind: "process"; name: string };

async function busyReason(tab: Tab): Promise<Busy | null> {
  if (tab.status !== "connected" || tab.sessionId == null) return null;
  if (tab.target.kind === "ssh") return { kind: "connected" };
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
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeKey, setActiveKey] = useState<number | null>(null);
  // undefined: dialog closed; null: creating a new profile.
  const [editing, setEditing] = useState<Profile | null | undefined>(undefined);
  const [importing, setImporting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Tabs waiting for the user to confirm closing them, with why the first busy one is busy.
  const [closing, setClosing] = useState<{ keys: number[]; busy: Busy; tab: Tab } | null>(null);
  // Title for local terminal tabs, e.g. "zsh".
  const [shellName, setShellName] = useState<string | null>(null);
  const nextKey = useRef(1);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const activeKeyRef = useRef(activeKey);
  activeKeyRef.current = activeKey;
  const localTitleRef = useRef("");
  localTitleRef.current = shellName ?? t("tabs.localTitle");

  const reloadProfiles = useCallback(() => {
    listProfiles().then(setProfiles).catch(console.error);
  }, []);
  useEffect(reloadProfiles, [reloadProfiles]);
  useEffect(() => {
    localShellName().then(setShellName).catch(console.error);
  }, []);

  const addTab = useCallback((target: SessionTarget, title: string) => {
    const key = nextKey.current++;
    setTabs((tabs) => [...tabs, newTab(key, target, title)]);
    setActiveKey(key);
  }, []);

  const openTab = (profile: Profile) => addTab({ kind: "ssh", profileId: profile.id }, profile.name);

  const openLocalTab = useCallback(() => addTab({ kind: "local" }, localTitleRef.current), [addTab]);

  // An SSH tab whose shell is up opens its copy on the same connection; anything else opens
  // the same target anew.
  const duplicateTab = (key: number) => {
    const tabs = tabsRef.current;
    const index = tabs.findIndex((t) => t.key === key);
    if (index < 0) return;
    const tab = tabs[index];
    const shareFrom = tab.target.kind === "ssh" && tab.status === "connected" ? (tab.sessionId ?? undefined) : undefined;
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
        t.key === activeKeyRef.current && t.target.kind === "ssh"
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
  }, [togglePanel, requestClose]);

  // From the macOS app menu's "Settings…" item (⌘,).
  useEffect(() => {
    const unlisten = listen("open-settings", () => setSettingsOpen(true));
    return () => void unlisten.then((f) => f());
  }, []);

  // From the macOS File menu's "New Local Terminal" item.
  useEffect(() => {
    const unlisten = listen("open-local-terminal", openLocalTab);
    return () => void unlisten.then((f) => f());
  }, [openLocalTab]);

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

  const closeDialog = useCallback(() => setEditing(undefined), []);
  const closeImport = useCallback(() => setImporting(false), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);

  return (
    <div className="app">
      <Sidebar
        profiles={profiles}
        onOpen={openTab}
        onEdit={setEditing}
        onNew={() => setEditing(null)}
        onImport={() => setImporting(true)}
        onLocalTerminal={openLocalTab}
        onSettings={() => setSettingsOpen(true)}
      />
      <main>
        {tabs.length > 0 && (
          <TabBar
            tabs={tabs}
            activeKey={activeKey}
            followRemoteTitle={settings.tabs.followRemoteTitle}
            onSelect={setActiveKey}
            onClose={(keys) => void requestClose(keys)}
            onMove={moveTab}
            onRename={(key, customTitle) => updateTab(key, { customTitle })}
            onDuplicate={duplicateTab}
            onReconnect={(key) =>
              setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, reconnectKey: t.reconnectKey + 1 } : t)))
            }
            onTogglePanel={togglePanel}
          />
        )}
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
      {editing !== undefined && (
        <ProfileDialog profile={editing} profiles={profiles} onClose={closeDialog} onChanged={reloadProfiles} />
      )}
      {importing && <ImportDialog onClose={closeImport} onImported={reloadProfiles} />}
      {settingsOpen && <SettingsDialog onClose={closeSettings} />}
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
