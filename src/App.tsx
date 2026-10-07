import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";

import { ImportDialog } from "./components/ImportDialog";
import { ProfileDialog } from "./components/ProfileDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { SessionPane } from "./components/SessionPane";
import { PANEL_SHORTCUTS, TabBar, type SidePanel, type Tab } from "./components/TabBar";
import type { SessionStatus } from "./components/TerminalView";
import { listProfiles, localShellName, type ForwardState, type Profile, type SessionTarget } from "./lib/api";
import { hasShiftShortcutModifiers, isSettingsShortcut } from "./lib/platform";
import "./styles.css";

const profileIdOf = (tab: Tab) => (tab.target.kind === "ssh" ? tab.target.profileId : undefined);

function App() {
  const { t } = useTranslation();
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeKey, setActiveKey] = useState<number | null>(null);
  // undefined: dialog closed; null: creating a new profile.
  const [editing, setEditing] = useState<Profile | null | undefined>(undefined);
  const [importing, setImporting] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
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
    setTabs((tabs) => [...tabs, { key, target, title, status: "connecting", sidePanel: null, forwards: {} }]);
    setActiveKey(key);
  }, []);

  const openTab = (profile: Profile) => addTab({ kind: "ssh", profileId: profile.id }, profile.name);

  const openLocalTab = useCallback(() => addTab({ kind: "local" }, localTitleRef.current), [addTab]);

  const closeTab = useCallback((key: number) => {
    const tabs = tabsRef.current;
    const index = tabs.findIndex((t) => t.key === key);
    if (index < 0) return;
    const remaining = tabs.filter((t) => t.key !== key);
    setTabs(remaining);
    if (key === activeKeyRef.current) {
      setActiveKey(remaining[Math.min(index, remaining.length - 1)]?.key ?? null);
    }
  }, []);

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
      const panel = PANEL_SHORTCUTS[e.code];
      if (!panel || !hasShiftShortcutModifiers(e)) return;
      // Capture phase, so the terminal never sees the keystroke.
      e.preventDefault();
      e.stopPropagation();
      togglePanel(panel);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [togglePanel]);

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

  const onStatus = (key: number, status: SessionStatus) =>
    // Forward events can precede "connected" (auto-start runs right after authentication),
    // so states are only reset when a connection attempt starts or ends.
    updateTab(key, status === "connected" ? { status } : { status, forwards: {} });

  // A local shell that exits cleanly (`exit`, Ctrl+D) closes its tab, like Terminal.app.
  const onExited = (tab: Tab, status: number | null) => {
    if (tab.target.kind === "local" && status === 0) closeTab(tab.key);
  };

  const onForward = (key: number, ruleId: string, state: ForwardState) =>
    setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, forwards: { ...t.forwards, [ruleId]: state } } : t)));

  const onProfileChanged = (updated: Profile) =>
    setProfiles((profiles) => profiles.map((p) => (p.id === updated.id ? updated : p)));

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
            onSelect={setActiveKey}
            onClose={closeTab}
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
              onForward={(ruleId, state) => onForward(tab.key, ruleId, state)}
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
    </div>
  );
}

export default App;
