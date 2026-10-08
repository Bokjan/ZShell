import { useRef, useState, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { ForwardState, SessionId, SessionTarget } from "../lib/api";
import { closeTabShortcutLabel, shiftShortcutLabel } from "../lib/platform";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import type { SessionStatus } from "./TerminalView";

export type SidePanel = "files" | "forwards";

export interface Tab {
  key: number;
  target: SessionTarget;
  /** The session name: the profile's, or the local shell's. */
  title: string;
  /** Set by renaming the tab; shown instead of any other title. */
  customTitle: string | null;
  /** Set by the shell (OSC 0 / 2); shown unless turned off in the settings. */
  remoteTitle: string | null;
  status: SessionStatus;
  /** The backend session, while there is one. */
  sessionId: SessionId | null;
  /** For a duplicated SSH tab: the session whose connection its first shell runs on. */
  shareFrom?: SessionId;
  /** Incremented to close the session and connect again. */
  reconnectKey: number;
  /** The side panel shown next to the terminal, if any (SSH tabs only). */
  sidePanel: SidePanel | null;
  /** Live state of the profile's forwarding rules on this tab's connection, by rule id. */
  forwards: Record<string, ForwardState>;
}

export const tabTitle = (tab: Tab, followRemoteTitle: boolean) =>
  tab.customTitle ?? ((followRemoteTitle && tab.remoteTitle) || tab.title);

/** Keyboard shortcut (with ⇧⌘ / Ctrl+Shift) toggling each side panel, by `KeyboardEvent.code`. */
export const PANEL_SHORTCUTS: Record<string, SidePanel> = { KeyE: "files", KeyP: "forwards" };

interface Props {
  tabs: Tab[];
  activeKey: number | null;
  followRemoteTitle: boolean;
  onSelect(key: number): void;
  /** Closes these tabs, asking first if needed. */
  onClose(keys: number[]): void;
  /** Moves the tab to `index` in the tab order. */
  onMove(key: number, index: number): void;
  /** `title` null goes back to the automatic title. */
  onRename(key: number, title: string | null): void;
  onDuplicate(key: number): void;
  onReconnect(key: number): void;
  onTogglePanel(panel: SidePanel): void;
}

/** How far the pointer moves before a press on a tab becomes a drag. */
const DRAG_THRESHOLD = 5;

export function TabBar({
  tabs,
  activeKey,
  followRemoteTitle,
  onSelect,
  onClose,
  onMove,
  onRename,
  onDuplicate,
  onReconnect,
  onTogglePanel,
}: Props) {
  const { t } = useTranslation();
  const navRef = useRef<HTMLElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [menu, setMenu] = useState<{ key: number; x: number; y: number } | null>(null);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const activeTab = tabs.find((t) => t.key === activeKey);
  const states = Object.values(activeTab?.forwards ?? {});
  const running = states.filter((s) => s.type === "starting" || s.type === "active").length;
  const failed = states.some((s) => s.type === "failed");
  // Files and forwards work on an SSH connection.
  const local = activeTab?.target.kind === "local";

  // Reorders live while dragging: the tab moves past each neighbor whose middle the pointer
  // crosses. Pressing doesn't take focus from the terminal.
  const startDrag = (e: ReactMouseEvent, key: number) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const startX = e.clientX;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved) {
        if (Math.abs(ev.clientX - startX) < DRAG_THRESHOLD) return;
        moved = true;
        setDragging(key);
      }
      const others = [...navRef.current!.querySelectorAll<HTMLElement>(".tab")].filter(
        (el) => el.dataset.key !== String(key),
      );
      const index = others.filter((el) => {
        const rect = el.getBoundingClientRect();
        return rect.left + rect.width / 2 < ev.clientX;
      }).length;
      if (tabsRef.current.findIndex((tab) => tab.key === key) !== index) onMove(key, index);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setDragging(null);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const menuItems = (key: number): MenuItem[] => {
    const index = tabs.findIndex((tab) => tab.key === key);
    const tab = tabs[index];
    const items: MenuItem[] = [
      { label: t("tabs.duplicate"), onSelect: () => onDuplicate(key) },
      { label: t("tabs.rename"), onSelect: () => setEditing(key) },
    ];
    if (tab.target.kind === "ssh") items.push({ label: t("tabs.reconnect"), onSelect: () => onReconnect(key) });
    items.push(
      "separator",
      { label: t("tabs.close"), shortcut: closeTabShortcutLabel, onSelect: () => onClose([key]) },
      {
        label: t("tabs.closeOthers"),
        disabled: tabs.length < 2,
        onSelect: () => onClose(tabs.filter((other) => other.key !== key).map((other) => other.key)),
      },
      {
        label: t("tabs.closeRight"),
        disabled: index === tabs.length - 1,
        onSelect: () => onClose(tabs.slice(index + 1).map((other) => other.key)),
      },
    );
    return items;
  };

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
    <nav className="tab-bar" ref={navRef}>
      {tabs.map((tab) => {
        const title = tabTitle(tab, followRemoteTitle);
        return (
          <div
            key={tab.key}
            data-key={tab.key}
            className={`tab${tab.key === activeKey ? " active" : ""}${tab.key === dragging ? " dragging" : ""}`}
            onMouseDown={(e) => editing !== tab.key && startDrag(e, tab.key)}
            onClick={() => onSelect(tab.key)}
            onDoubleClick={() => setEditing(tab.key)}
            onAuxClick={(e) => e.button === 1 && onClose([tab.key])}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ key: tab.key, x: e.clientX, y: e.clientY });
            }}
            title={editing === tab.key ? undefined : `${title}\n${t("tabs.renameHint")}`}
          >
            <span className={`status-dot ${tab.status}`} />
            {editing === tab.key ? (
              <TitleEditor
                initial={title}
                onDone={(value) => {
                  setEditing(null);
                  if (value !== null && value !== title) onRename(tab.key, value || null);
                }}
              />
            ) : (
              <span className="tab-title">{title}</span>
            )}
            <button
              className="tab-close"
              title={`${t("tabs.close")} (${closeTabShortcutLabel})`}
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                onClose([tab.key]);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
      {menu && tabs.some((tab) => tab.key === menu.key) && (
        <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.key)} onClose={() => setMenu(null)} />
      )}
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

/**
 * Inline editor for a tab's title. Enter or leaving the field keeps the text (empty goes back
 * to the automatic title); Escape cancels with `null`.
 */
function TitleEditor({ initial, onDone }: { initial: string; onDone(value: string | null): void }) {
  const done = useRef(false);
  const finish = (value: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(value);
  };
  return (
    <input
      className="tab-title-input"
      defaultValue={initial}
      autoFocus
      onFocus={(e) => e.target.select()}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(e.currentTarget.value.trim());
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish(e.target.value.trim())}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
    />
  );
}
