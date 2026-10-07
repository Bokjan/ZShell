import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { ForwardState, SessionTarget } from "../lib/api";
import { shiftShortcutLabel } from "../lib/platform";
import type { SessionStatus } from "./TerminalView";

export type SidePanel = "files" | "forwards";

export interface Tab {
  key: number;
  target: SessionTarget;
  title: string;
  status: SessionStatus;
  /** The side panel shown next to the terminal, if any (SSH tabs only). */
  sidePanel: SidePanel | null;
  /** Live state of the profile's forwarding rules on this tab's connection, by rule id. */
  forwards: Record<string, ForwardState>;
}

/** Keyboard shortcut (with ⇧⌘ / Ctrl+Shift) toggling each side panel, by `KeyboardEvent.code`. */
export const PANEL_SHORTCUTS: Record<string, SidePanel> = { KeyE: "files", KeyP: "forwards" };

interface Props {
  tabs: Tab[];
  activeKey: number | null;
  onSelect(key: number): void;
  onClose(key: number): void;
  onTogglePanel(panel: SidePanel): void;
}

export function TabBar({ tabs, activeKey, onSelect, onClose, onTogglePanel }: Props) {
  const { t } = useTranslation();
  const activeTab = tabs.find((t) => t.key === activeKey);
  const states = Object.values(activeTab?.forwards ?? {});
  const running = states.filter((s) => s.type === "starting" || s.type === "active").length;
  const failed = states.some((s) => s.type === "failed");
  // Files and forwards work on an SSH connection.
  const local = activeTab?.target.kind === "local";

  const segment = (panel: SidePanel, label: string, hint: string, badge?: ReactNode) => (
    <button
      className={activeTab?.sidePanel === panel ? "on" : undefined}
      disabled={!activeTab || local}
      onClick={() => onTogglePanel(panel)}
      title={local ? t("tabs.panelUnavailable") : hint}
    >
      {label}
      {badge}
    </button>
  );

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
            title={t("tabs.close")}
            onClick={(e) => {
              e.stopPropagation();
              onClose(tab.key);
            }}
          >
            ×
          </button>
        </div>
      ))}
      <span className="grow" />
      <div className="panel-switch">
        {segment("files", t("tabs.files"), t("tabs.filesHint", { shortcut: shiftShortcutLabel("E") }))}
        {segment(
          "forwards",
          t("tabs.forwards"),
          failed
            ? t("tabs.forwardsHintFailed", { shortcut: shiftShortcutLabel("P") })
            : t("tabs.forwardsHint", { shortcut: shiftShortcutLabel("P") }),
          failed ? <span className="badge failed" /> : running > 0 && <span className="badge">{running}</span>,
        )}
      </div>
    </nav>
  );
}
