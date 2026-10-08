import {
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { useTranslation } from "react-i18next";

import type { ForwardState, Protocol, SessionId, SessionTarget } from "../lib/api";
import { closeTabShortcutLabel, isWindows, newTabShortcutLabel, shiftShortcutLabel } from "../lib/platform";
import { DRAG_REGION } from "../lib/window";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { ComposeIcon, PlusIcon, QuickIcon } from "./icons";
import type { SessionStatus } from "./TerminalView";
import { WindowControls } from "./WindowControls";

export type SidePanel = "files" | "forwards";

/** What a tab runs: a remote session's protocol, or a local terminal. */
export type TabProtocol = Protocol | "local";

export interface Tab {
  key: number;
  target: SessionTarget;
  /** Of the current (or last) session; a saved session's may change between connections. */
  protocol: TabProtocol;
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
  /** The quick command group picked in this tab; null shows its session's. */
  commandGroup: string | null;
  /** The log this tab writes, kept across reconnections (which go on with it). */
  logPath: string | null;
  /** Logging was stopped by hand: reconnections don't start it again. */
  logStopped: boolean;
}

/** Whether the tab is writing its log now. */
export const isLogging = (tab: Tab) => tab.logPath !== null && tab.status !== "closed";

export const tabTitle = (tab: Tab, followRemoteTitle: boolean) =>
  tab.customTitle ?? ((followRemoteTitle && tab.remoteTitle) || tab.title);

/** Keyboard shortcut (with ⇧⌘ / Ctrl+Shift) toggling each side panel, by `KeyboardEvent.code`. */
export const PANEL_SHORTCUTS: Record<string, SidePanel> = { KeyE: "files", KeyP: "forwards" };

interface Props {
  tabs: Tab[];
  activeKey: number | null;
  followRemoteTitle: boolean;
  onSelect(key: number): void;
  /** Opens a local terminal tab ("+"); absent when the system doesn't allow them. */
  onNew?(): void;
  /** Closes these tabs, asking first if needed. */
  onClose(keys: number[]): void;
  /** Moves the tab to `index` in the tab order. */
  onMove(key: number, index: number): void;
  /** `title` null goes back to the automatic title. */
  onRename(key: number, title: string | null): void;
  onDuplicate(key: number): void;
  /** Saves a quick connection tab as a session. */
  onSaveAsSession(key: number): void;
  onReconnect(key: number): void;
  /** Sends a break (serial and Telnet tabs). */
  onBreak(key: number): void;
  /** Starts (`true`) or stops logging the tab's session. */
  onLog(key: number, start: boolean): void;
  onShowLog(key: number): void;
  onTogglePanel(panel: SidePanel): void;
  composeOpen: boolean;
  onToggleCompose(): void;
  quickBarOpen: boolean;
  onToggleQuickBar(): void;
  /** Tabs that the compose bar sends to besides (or instead of) the active one; marked. */
  inScope: number[];
  /** Typing is synced to the tabs in scope; their marks turn to the warning color. */
  syncing: boolean;
  /** Tabs that just received text from the compose bar or a quick command; they flash. */
  flashing: number[];
  /** The color a tab is marked with (its session's background color), if any. */
  colorOf(tab: Tab): string | undefined;
  /** Where the tab's session connects (see `address`), for its tooltip; none for local terminals. */
  addressOf(tab: Tab): string | undefined;
}

/** How far the pointer moves before a press on a tab becomes a drag. */
const DRAG_THRESHOLD = 5;
/** While dragging a tab this close to an edge of the strip, the strip scrolls. */
const DRAG_SCROLL_EDGE = 24;
const DRAG_SCROLL_STEP = 12;

export function TabBar({
  tabs,
  activeKey,
  followRemoteTitle,
  onSelect,
  onNew,
  onClose,
  onMove,
  onRename,
  onDuplicate,
  onSaveAsSession,
  onReconnect,
  onBreak,
  onLog,
  onShowLog,
  onTogglePanel,
  composeOpen,
  onToggleCompose,
  quickBarOpen,
  onToggleQuickBar,
  inScope,
  syncing,
  flashing,
  colorOf,
  addressOf,
}: Props) {
  const { t } = useTranslation();
  // Tabs that don't fit scroll sideways, without a scroll bar (see `.tab-strip`).
  const stripRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [menu, setMenu] = useState<{ key: number; x: number; y: number } | null>(null);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;

  // Keep the active tab in view, e.g. after Ctrl+Tab or opening a tab.
  useEffect(() => {
    const strip = stripRef.current;
    const tab = strip?.querySelector<HTMLElement>(`.tab[data-key="${activeKey}"]`);
    if (!strip || !tab) return;
    if (tab.offsetLeft < strip.scrollLeft) strip.scrollLeft = tab.offsetLeft;
    else if (tab.offsetLeft + tab.offsetWidth > strip.scrollLeft + strip.clientWidth) {
      strip.scrollLeft = tab.offsetLeft + tab.offsetWidth - strip.clientWidth;
    }
  }, [activeKey, tabs.length]);

  // A mouse wheel scrolls the strip sideways; trackpads already scroll horizontally.
  const onWheel = (e: ReactWheelEvent) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) stripRef.current!.scrollLeft += e.deltaY;
  };
  const activeTab = tabs.find((t) => t.key === activeKey);
  const states = Object.values(activeTab?.forwards ?? {});
  const running = states.filter((s) => s.type === "starting" || s.type === "active").length;
  const failed = states.some((s) => s.type === "failed");
  // Files and forwards work on an SSH connection.
  const unavailable = !!activeTab && activeTab.protocol !== "ssh";

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
      const strip = stripRef.current!;
      const bounds = strip.getBoundingClientRect();
      if (ev.clientX < bounds.left + DRAG_SCROLL_EDGE) strip.scrollLeft -= DRAG_SCROLL_STEP;
      else if (ev.clientX > bounds.right - DRAG_SCROLL_EDGE) strip.scrollLeft += DRAG_SCROLL_STEP;
      const others = [...strip.querySelectorAll<HTMLElement>(".tab")].filter(
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
    const items: MenuItem[] = [];
    // A serial device can only be open once.
    if (tab.protocol !== "serial") items.push({ label: t("tabs.duplicate"), onSelect: () => onDuplicate(key) });
    items.push({ label: t("tabs.rename"), onSelect: () => setEditing(key) });
    if (tab.protocol !== "local") items.push({ label: t("tabs.reconnect"), onSelect: () => onReconnect(key) });
    if (tab.protocol === "serial" || tab.protocol === "telnet") {
      items.push({ label: t("tabs.sendBreak"), disabled: tab.status !== "connected", onSelect: () => onBreak(key) });
    }
    if (tab.target.kind === "quick") items.push({ label: t("tabs.saveAsSession"), onSelect: () => onSaveAsSession(key) });
    items.push("separator");
    if (isLogging(tab)) items.push({ label: t("tabs.stopLog"), onSelect: () => onLog(key, false) });
    else items.push({ label: t("tabs.startLog"), disabled: tab.status !== "connected", onSelect: () => onLog(key, true) });
    if (tab.logPath) items.push({ label: t("tabs.showLog"), onSelect: () => onShowLog(key) });
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
      disabled={!activeTab || unavailable}
      onClick={() => onTogglePanel(panel)}
      title={unavailable ? t("tabs.panelUnavailable") : hint}
    >
      {label}
      {badge}
    </button>
  );

  // The window's title bar: always shown, so "+" and room to move the window stay available.
  return (
    <nav className="tab-bar" {...DRAG_REGION}>
      <div className="tab-strip" ref={stripRef} onWheel={onWheel}>
        {tabs.map((tab) => {
          const title = tabTitle(tab, followRemoteTitle);
          const color = colorOf(tab);
          const where = addressOf(tab);
          return (
            <div
              key={tab.key}
              data-key={tab.key}
              className={[
                "tab",
                tab.key === activeKey && "active",
                tab.key === dragging && "dragging",
                inScope.includes(tab.key) && (syncing ? "in-scope syncing" : "in-scope"),
                flashing.includes(tab.key) && "flash",
              ]
                .filter(Boolean)
                .join(" ")}
              onMouseDown={(e) => editing !== tab.key && startDrag(e, tab.key)}
              onClick={() => onSelect(tab.key)}
              onDoubleClick={() => setEditing(tab.key)}
              onAuxClick={(e) => e.button === 1 && onClose([tab.key])}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ key: tab.key, x: e.clientX, y: e.clientY });
              }}
              title={editing === tab.key ? undefined : [title, where, t("tabs.renameHint")].filter(Boolean).join("\n")}
            >
              {color && <span className="tab-color" style={{ background: color }} />}
              <span className={`status-dot ${tab.status}`} />
              {isLogging(tab) && <span className="tab-log" title={t("tabs.logging", { path: tab.logPath })} />}
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
      </div>
      {menu && tabs.some((tab) => tab.key === menu.key) && (
        <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.key)} onClose={() => setMenu(null)} />
      )}
      {onNew && (
        <button
          className="tab-new"
          title={t("tabs.newLocalTerminal", { shortcut: newTabShortcutLabel })}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onNew}
        >
          <PlusIcon />
        </button>
      )}
      {/* Grows, and keeps some room to move the window however many tabs there are. */}
      <span className="tab-bar-drag" {...DRAG_REGION} />
      {tabs.length > 0 && (
        <button
          className={`compose-toggle${composeOpen ? " on" : ""}`}
          title={t("compose.toggle", { shortcut: shiftShortcutLabel("I") })}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onToggleCompose}
        >
          <ComposeIcon />
        </button>
      )}
      {tabs.length > 0 && (
        <button
          className={`compose-toggle${quickBarOpen ? " on" : ""}`}
          title={t("quick.toggle")}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onToggleQuickBar}
        >
          <QuickIcon />
        </button>
      )}
      {tabs.length > 0 && (
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
      )}
      {isWindows && <WindowControls />}
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
