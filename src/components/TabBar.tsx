import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { useTranslation } from "react-i18next";

import { focusedPane, isLogging, tabId, tabPanelId, tabTitle, type SidePanel, type Tab } from "../lib/panes";
import type { Direction } from "../lib/layout";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import { shortcutLabel } from "../lib/keymap";
import { isComposing, isMac } from "../lib/platform";
import type { Point, TabDragNativeEnd } from "../lib/bindings";
import { tabIndexAt } from "../lib/tabStrip";
import { DRAG_REGION } from "../lib/window";
import { listenHere, tabDrag, useWindowEvent, type DropTarget, type WindowInfo } from "../lib/windows";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { IconButton } from "./IconButton";
import { CloseIcon, ComposeIcon, PlusIcon, QuickIcon } from "./icons";
import { WindowControls } from "./WindowControls";

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
  /** The other windows, which a tab can move to. */
  windows: WindowInfo[];
  /** Whether the tab can move to another window now. */
  canMoveToWindow(key: number): boolean;
  /** Moves the tab to window `target`, or to a new window (null). */
  onMoveToWindow(key: number, target: string | null): void;
  /**
   * The tab was dragged out of the window and dropped on `target`, `grab`bed that far from the
   * top left of the window.
   */
  onDropOut(key: number, target: DropTarget, grab: Point): void;
  /** `title` null goes back to the automatic title. */
  onRename(key: number, title: string | null): void;
  onDuplicate(key: number): void;
  /** Splits the tab's focused pane. */
  onSplit(key: number, direction: Direction): void;
  /** Whether the tab's focused pane is large enough to split. */
  canSplit(key: number, direction: Direction): boolean;
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
  windows,
  canMoveToWindow,
  onMoveToWindow,
  onDropOut,
  onSplit,
  canSplit,
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
  const barRef = useRef<HTMLElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  // The dragged tab would leave the window if dropped now.
  const [leaving, setLeaving] = useState(false);
  // Where a tab dragged from another window would go, while it is over this tab bar.
  const [dropX, setDropX] = useState<number | null>(null);
  useWindowEvent<number | null>("tab-drag-over", setDropX);
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

  // Where the tab bar is, for tabs dragged from other windows (see `tab_drag/`).
  useEffect(() => {
    const bar = barRef.current!;
    const report = () => {
      const { x, y, width, height } = bar.getBoundingClientRect();
      void tabDrag.setTabBar({ x, y, width, height }).catch(console.error);
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(bar);
    window.addEventListener("resize", report);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", report);
    };
  }, []);

  // A mouse wheel scrolls the strip sideways; trackpads already scroll horizontally.
  const onWheel = (e: ReactWheelEvent) => {
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) stripRef.current!.scrollLeft += e.deltaY;
  };
  const activeTab = tabs.find((t) => t.key === activeKey);
  const activePane = activeTab && focusedPane(activeTab);
  const states = Object.values(activePane?.forwards ?? {});
  const running = states.filter((s) => s.type === "starting" || s.type === "active").length;
  const failed = states.some((s) => s.type === "failed");
  // Files and forwards work on an SSH connection.
  const unavailable = !!activePane && activePane.protocol !== "ssh";

  // Reorders live while dragging: the tab moves past each neighbor whose middle the pointer
  // crosses. Pressing doesn't take focus from the terminal. Mouse events rather than HTML5
  // drag and drop, which Tauri's file drop handling intercepts on Windows. Dragged out of the
  // window, the tab goes where the backend says it is over when dropped (another window's tab
  // bar, or a window of its own); Escape puts it back.
  const startDrag = (e: ReactMouseEvent, key: number) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const grab = { x: e.clientX, y: e.clientY };
    // Names the drag for the backend, which ignores answers that come back after it ended.
    const id = crypto.randomUUID();
    const startIndex = tabsRef.current.findIndex((tab) => tab.key === key);
    let moved = false;
    // Whether it can go to another window, as the drag starts.
    let movable = false;
    // One question about where it would go at a time, and the latest position after it.
    let asking = false;
    let askAgain = false;
    let ended = false;
    // Where the pointer last was: outside the page or not.
    let outside = false;
    // The system's drag took over (Wayland): the page hears no more of the mouse, and waits
    // for the end of that drag.
    let native = false;
    const isOutside = (ev: MouseEvent) =>
      ev.clientX < 0 || ev.clientY < 0 || ev.clientX >= window.innerWidth || ev.clientY >= window.innerHeight;
    const ask = () => {
      if (asking) {
        askAgain = true;
        return;
      }
      asking = true;
      void tabDrag
        .move(id, outside)
        .then((target) => {
          if (target.kind === "native") goNative();
          // A window's only tab has nowhere new to go.
          const leaves = target.kind === "window" || (target.kind !== "here" && tabsRef.current.length > 1);
          if (!ended) setLeaving(leaves);
        }, console.error)
        .finally(() => {
          asking = false;
          if (askAgain && !ended) {
            askAgain = false;
            ask();
          }
        });
    };
    const move = (ev: MouseEvent) => {
      outside = isOutside(ev);
      // Released where the page didn't hear it.
      if (ev.buttons === 0) {
        finish(true);
        return;
      }
      if (!moved) {
        // Any way: straight down out of the window too.
        if (Math.hypot(ev.clientX - grab.x, ev.clientY - grab.y) < DRAG_THRESHOLD) return;
        moved = true;
        movable = canMoveToWindow(key);
        setDragging(key);
      }
      if (movable) ask();
      if (outside) return;
      const strip = stripRef.current!;
      const bounds = strip.getBoundingClientRect();
      if (ev.clientX < bounds.left + DRAG_SCROLL_EDGE) strip.scrollLeft -= DRAG_SCROLL_STEP;
      else if (ev.clientX > bounds.right - DRAG_SCROLL_EDGE) strip.scrollLeft += DRAG_SCROLL_STEP;
      const index = tabIndexAt(strip, ev.clientX, key);
      if (tabsRef.current.findIndex((tab) => tab.key === key) !== index) onMove(key, index);
    };
    const keyDown = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      if (moved) onMove(key, startIndex);
      finish(false);
    };
    const goNative = () => {
      if (native || ended) return;
      native = true;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", keyDown, true);
      const unlisten = listenHere<TabDragNativeEnd>("tab-drag-native-end", ({ payload }) => {
        if (payload.id !== id) return;
        void unlisten.then((f) => f());
        // Cancelled (Escape): back where it was, as in the page.
        if (!payload.dropped && moved) onMove(key, startIndex);
        finish(payload.dropped);
      });
    };
    const finish = (dropped: boolean) => {
      if (ended) return;
      ended = true;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", keyDown, true);
      setDragging(null);
      setLeaving(false);
      if (!movable) return;
      void tabDrag.end(id, dropped, outside).then((target) => target && onDropOut(key, target, grab), console.error);
    };
    const up = (ev: MouseEvent) => {
      outside = isOutside(ev);
      finish(true);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", keyDown, true);
  };

  // Where a tab dragged from another window would go among the tabs.
  const dropMarker = (() => {
    const strip = stripRef.current;
    if (dropX === null || !strip) return null;
    const elements = [...strip.querySelectorAll<HTMLElement>(".tab")];
    const index = tabIndexAt(strip, dropX);
    const last = elements[elements.length - 1];
    const left = index < elements.length ? elements[index].offsetLeft : last ? last.offsetLeft + last.offsetWidth : 0;
    return <span className="tab-drop-marker" style={{ left }} aria-hidden="true" />;
  })();

  const menuItems = (key: number): MenuItem[] => {
    const index = tabs.findIndex((tab) => tab.key === key);
    // Session actions act on the focused pane.
    const pane = focusedPane(tabs[index]);
    const split = tabs[index].panes.length > 1;
    const items: MenuItem[] = [];
    // A serial device can only be open once.
    if (pane.protocol !== "serial") {
      items.push(
        { label: t("tabs.duplicate"), onSelect: () => onDuplicate(key) },
        {
          label: t("tabs.splitRight"),
          shortcut: shortcutLabel("splitRight"),
          disabled: !canSplit(key, "row"),
          onSelect: () => onSplit(key, "row"),
        },
        {
          label: t("tabs.splitDown"),
          shortcut: shortcutLabel("splitDown"),
          disabled: !canSplit(key, "column"),
          onSelect: () => onSplit(key, "column"),
        },
      );
    }
    items.push({ label: t("tabs.rename"), onSelect: () => setEditing(key) });
    if (pane.protocol !== "local") items.push({ label: t("tabs.reconnect"), onSelect: () => onReconnect(key) });
    if (pane.protocol === "serial" || pane.protocol === "telnet") {
      items.push({ label: t("tabs.sendBreak"), disabled: pane.status !== "connected", onSelect: () => onBreak(key) });
    }
    if (pane.target.kind === "quick") items.push({ label: t("tabs.saveAsSession"), onSelect: () => onSaveAsSession(key) });
    items.push("separator");
    if (isLogging(pane)) items.push({ label: t("tabs.stopLog"), onSelect: () => onLog(key, false) });
    else items.push({ label: t("tabs.startLog"), disabled: pane.status !== "connected", onSelect: () => onLog(key, true) });
    if (pane.logPath) items.push({ label: t("tabs.showLog"), onSelect: () => onShowLog(key) });
    // A window's only tab has nowhere new to go.
    const movable = canMoveToWindow(key);
    items.push(
      "separator",
      { label: t("tabs.moveToNewWindow"), disabled: !movable || tabs.length < 2, onSelect: () => onMoveToWindow(key, null) },
      ...windows.map((other) => ({
        label: t("tabs.moveToWindow", { name: other.title || t("app.name") }),
        disabled: !movable,
        onSelect: () => onMoveToWindow(key, other.label),
      })),
    );
    items.push(
      "separator",
      // ⌘W closes the focused pane of a split tab.
      { label: t("tabs.close"), shortcut: split ? undefined : shortcutLabel("closeTab"), onSelect: () => onClose([key]) },
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

  /**
   * Keys on a focused tab: arrows, Home and End move among the tabs, Enter or Space shows
   * one (and its terminal takes the focus), F2 renames, Shift+F10 or the menu key opens its
   * menu.
   */
  const onTabKeyDown = (e: ReactKeyboardEvent<HTMLElement>, index: number) => {
    if (editing !== null || isComposing(e)) return;
    const key = tabs[index].key;
    let next: number;
    if (e.key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (e.key === "ArrowLeft") next = (index - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = tabs.length - 1;
    else {
      if (e.key === "Enter" || e.key === " ") onSelect(key);
      else if (e.key === "F2") setEditing(key);
      else if (isMenuKey(e)) {
        openedByMenuKey();
        const rect = e.currentTarget.getBoundingClientRect();
        setMenu({ key, x: rect.left, y: rect.bottom });
      } else return;
      e.preventDefault();
      return;
    }
    e.preventDefault();
    document.getElementById(tabId(tabs[next].key))?.focus();
  };

  /** Whether a pane of the tab has a question waiting in its file panel, out of sight. */
  const questionWaiting = (tab: Tab) =>
    tab.panes.some((p) => p.questions > 0 && !(tab.key === activeKey && tab.focused === p.key && tab.sidePanel === "files"));

  const statusLabel = (tab: Tab) => {
    const pane = focusedPane(tab);
    const question = questionWaiting(tab) ? t("tabs.questionWaiting") : null;
    if (pane.status === "connecting") return t("tabs.status.connecting");
    if (pane.status !== "closed") return question;
    const closed = t(pane.protocol === "local" ? "tabs.status.exited" : "tabs.status.closed");
    return question ? `${closed}. ${question}` : closed;
  };

  const segment = (panel: SidePanel, label: string, hint: string, badge?: ReactNode) => (
    <button
      className={activeTab?.sidePanel === panel ? "on" : undefined}
      aria-pressed={activeTab?.sidePanel === panel}
      disabled={!activeTab || (unavailable && activeTab.sidePanel !== panel)}
      onClick={() => onTogglePanel(panel)}
      title={unavailable ? t("tabs.panelUnavailable") : hint}
    >
      {label}
      {badge}
    </button>
  );

  // The window's title bar: always shown, so "+" and room to move the window stay available.
  return (
    <nav className="tab-bar" ref={barRef} {...DRAG_REGION}>
      <div className="tab-strip" ref={stripRef} onWheel={onWheel} role="tablist" aria-label={t("tabs.listLabel")}>
        {tabs.map((tab, index) => {
          const pane = focusedPane(tab);
          const title = tabTitle(tab, followRemoteTitle);
          const color = colorOf(tab);
          const where = addressOf(tab);
          const status = statusLabel(tab);
          return (
            <div
              key={tab.key}
              data-key={tab.key}
              id={tabId(tab.key)}
              role="tab"
              aria-selected={tab.key === activeKey}
              aria-controls={tabPanelId(tab.key)}
              // The active tab is the one Tab reaches; arrows move to the others.
              tabIndex={tab.key === activeKey ? 0 : -1}
              onKeyDown={(e) => onTabKeyDown(e, index)}
              className={[
                "tab",
                tab.key === activeKey && "active",
                tab.key === dragging && (leaving ? "dragging leaving" : "dragging"),
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
                if (followsMenuKey(e)) return;
                e.preventDefault();
                setMenu({ key: tab.key, x: e.clientX, y: e.clientY });
              }}
              title={editing === tab.key ? undefined : [title, where, t("tabs.renameHint")].filter(Boolean).join("\n")}
            >
              {color && <span className="tab-color" style={{ background: color }} />}
              <span className={`status-dot ${pane.status}`} />
              {isLogging(pane) && <span className="tab-log" title={t("tabs.logging", { path: pane.logPath })} />}
              {questionWaiting(tab) && <span className="tab-question" title={t("tabs.questionWaiting")} />}
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
              {status && <span className="visually-hidden">{status}</span>}
              {/* For the mouse: the keyboard closes tabs with the shortcut or the tab's menu. */}
              <button
                className="tab-close"
                tabIndex={-1}
                aria-hidden="true"
                title={tab.panes.length > 1 ? t("tabs.close") : t("tabs.closeWithShortcut", { shortcut: shortcutLabel("closeTab") })}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onClose([tab.key]);
                }}
              >
                <CloseIcon size={12} />
              </button>
            </div>
          );
        })}
        {dropMarker}
      </div>
      {menu && tabs.some((tab) => tab.key === menu.key) && (
        <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.key)} onClose={() => setMenu(null)} />
      )}
      {onNew && (
        <IconButton
          className="tab-new"
          label={t("tabs.newLocalTerminal", { shortcut: shortcutLabel("newLocalTerminal") })}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onNew}
        >
          <PlusIcon />
        </IconButton>
      )}
      {/* Grows, and keeps some room to move the window however many tabs there are. */}
      <span className="tab-bar-drag" {...DRAG_REGION} />
      {tabs.length > 0 && (
        <IconButton
          className={`compose-toggle${composeOpen ? " on" : ""}`}
          label={t("compose.toggle", { shortcut: shortcutLabel("composeBar") })}
          aria-pressed={composeOpen}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onToggleCompose}
        >
          <ComposeIcon />
        </IconButton>
      )}
      {tabs.length > 0 && (
        <IconButton
          className={`compose-toggle${quickBarOpen ? " on" : ""}`}
          label={t("quick.toggle")}
          aria-pressed={quickBarOpen}
          onMouseDown={(e) => e.preventDefault()}
          onClick={onToggleQuickBar}
        >
          <QuickIcon />
        </IconButton>
      )}
      {tabs.length > 0 && (
        <div className="panel-switch">
          {segment(
            "files",
            t("tabs.files"),
            activeTab && questionWaiting(activeTab)
              ? t("tabs.filesHintQuestion", { shortcut: shortcutLabel("filePanel") })
              : t("tabs.filesHint", { shortcut: shortcutLabel("filePanel") }),
            activeTab && questionWaiting(activeTab) && <span className="badge question" />,
          )}
          {segment(
            "forwards",
            t("tabs.forwards"),
            failed
              ? t("tabs.forwardsHintFailed", { shortcut: shortcutLabel("forwardsPanel") })
              : t("tabs.forwardsHint", { shortcut: shortcutLabel("forwardsPanel") }),
            failed ? <span className="badge failed" /> : running > 0 && <span className="badge">{running}</span>,
          )}
        </div>
      )}
      {!isMac && <WindowControls />}
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
        if (isComposing(e)) return;
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
