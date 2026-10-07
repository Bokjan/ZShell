import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ProfileDialog } from "./components/ProfileDialog";
import { Sidebar } from "./components/Sidebar";
import { SessionPane } from "./components/SessionPane";
import { PANEL_SHORTCUTS, TabBar, type SidePanel, type Tab } from "./components/TabBar";
import type { SessionStatus } from "./components/TerminalView";
import { listProfiles, type ForwardState, type Profile } from "./lib/api";
import { hasShiftShortcutModifiers } from "./lib/platform";
import "./styles.css";

function App() {
  const { t } = useTranslation();
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeKey, setActiveKey] = useState<number | null>(null);
  // undefined: dialog closed; null: creating a new profile.
  const [editing, setEditing] = useState<Profile | null | undefined>(undefined);
  const nextKey = useRef(1);
  const activeKeyRef = useRef(activeKey);
  activeKeyRef.current = activeKey;

  const reloadProfiles = useCallback(() => {
    listProfiles().then(setProfiles).catch(console.error);
  }, []);
  useEffect(reloadProfiles, [reloadProfiles]);

  const openTab = (profile: Profile) => {
    const key = nextKey.current++;
    setTabs((tabs) => [...tabs, { key, profileId: profile.id, title: profile.name, status: "connecting", sidePanel: null, forwards: {} }]);
    setActiveKey(key);
  };

  const closeTab = (key: number) => {
    const index = tabs.findIndex((t) => t.key === key);
    const remaining = tabs.filter((t) => t.key !== key);
    setTabs(remaining);
    if (key === activeKey) {
      setActiveKey(remaining[Math.min(index, remaining.length - 1)]?.key ?? null);
    }
  };

  const updateTab = (key: number, patch: Partial<Tab>) =>
    setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, ...patch } : t)));

  const togglePanel = useCallback((panel: SidePanel) => {
    setTabs((tabs) =>
      tabs.map((t) =>
        t.key === activeKeyRef.current ? { ...t, sidePanel: t.sidePanel === panel ? null : panel } : t,
      ),
    );
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
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

  const onStatus = (key: number, status: SessionStatus) =>
    // Forward events can precede "connected" (auto-start runs right after authentication),
    // so states are only reset when a connection attempt starts or ends.
    updateTab(key, status === "connected" ? { status } : { status, forwards: {} });

  const onForward = (key: number, ruleId: string, state: ForwardState) =>
    setTabs((tabs) => tabs.map((t) => (t.key === key ? { ...t, forwards: { ...t.forwards, [ruleId]: state } } : t)));

  const onProfileChanged = (updated: Profile) =>
    setProfiles((profiles) => profiles.map((p) => (p.id === updated.id ? updated : p)));

  const closeDialog = useCallback(() => setEditing(undefined), []);

  return (
    <div className="app">
      <Sidebar profiles={profiles} onOpen={openTab} onEdit={setEditing} onNew={() => setEditing(null)} />
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
              profile={profiles.find((p) => p.id === tab.profileId)}
              active={tab.key === activeKey}
              onStatus={(status) => onStatus(tab.key, status)}
              onForward={(ruleId, state) => onForward(tab.key, ruleId, state)}
              onProfileChanged={onProfileChanged}
            />
          ))}
          {tabs.length === 0 && <div className="placeholder">{t("app.placeholder")}</div>}
        </div>
      </main>
      {editing !== undefined && <ProfileDialog profile={editing} onClose={closeDialog} onChanged={reloadProfiles} />}
    </div>
  );
}

export default App;
