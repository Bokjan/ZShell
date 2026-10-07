import type { SessionStatus } from "./TerminalView";

export interface Tab {
  key: number;
  profileId: string;
  title: string;
  status: SessionStatus;
}

interface Props {
  tabs: Tab[];
  activeKey: number | null;
  onSelect(key: number): void;
  onClose(key: number): void;
}

export function TabBar({ tabs, activeKey, onSelect, onClose }: Props) {
  return (
    <nav className="tab-bar">
      {tabs.map((tab) => (
        <div
          key={tab.key}
          className={`tab${tab.key === activeKey ? " active" : ""}`}
          onClick={() => onSelect(tab.key)}
          onAuxClick={(e) => e.button === 1 && onClose(tab.key)}
          title={tab.title}
        >
          <span className={`status-dot ${tab.status}`} />
          <span className="tab-title">{tab.title}</span>
          <button
            className="tab-close"
            title="关闭"
            onClick={(e) => {
              e.stopPropagation();
              onClose(tab.key);
            }}
          >
            ×
          </button>
        </div>
      ))}
    </nav>
  );
}
