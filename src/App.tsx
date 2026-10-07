import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ProfileDialog } from "./components/ProfileDialog";
import { Sidebar } from "./components/Sidebar";
import { SessionPane } from "./components/SessionPane";
import { TabBar, type Tab } from "./components/TabBar";
import type { SessionStatus } from "./components/TerminalView";
import { listProfiles, type Profile } from "./lib/api";
import "./styles.css";

function App() {
  const { t } = useTranslation();
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeKey, setActiveKey] = useState<number | null>(null);
  // undefined: dialog closed; null: creating a new profile.
  const [editing, setEditing] = useState<Profile | null | undefined>(undefined);
  const nextKey = useRef(1);

  const reloadProfiles = useCallback(() => {
    listProfiles().then(setProfiles).catch(console.error);
  }, []);
  useEffect(reloadProfiles, [reloadProfiles]);

  const openTab = (profile: Profile) => {
    const key = nextKey.current++;
    setTabs((tabs) => [...tabs, { key, profileId: profile.id, title: profile.name, status: "connecting", filesOpen: false }]);
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
            onToggleFiles={() => {
              const tab = tabs.find((t) => t.key === activeKey);
              if (tab) updateTab(tab.key, { filesOpen: !tab.filesOpen });
            }}
          />
        )}
        <div className="terminals">
          {tabs.map((tab) => (
            <SessionPane
              key={tab.key}
              tab={tab}
              active={tab.key === activeKey}
              onStatus={(status: SessionStatus) => updateTab(tab.key, { status })}
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
