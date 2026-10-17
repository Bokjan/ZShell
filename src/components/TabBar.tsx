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
import { isComposing, isMac } from "../lib/platform";
import type { Point } from "../ipc/bindings";
import { tabIndexAt } from "../lib/tabStrip";
import { DRAG_REGION } from "../hooks/titleBar";
import { tabDrag, type DropTarget, type WindowInfo } from "../ipc/windows";
import { useWindowEvent } from "../hooks/windowEvents";
import { useLatest } from "../hooks/latest";
import { useLocale } from "../hooks/locale";
import { useStableHandlers } from "../hooks/stableHandlers";
import { useStableProps } from "../hooks/stableProps";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { IconButton } from "./IconButton";
import { BellIcon, CloseIcon, ComposeIcon, PlusIcon, QuickIcon } from "./icons";
import { useTabDrag } from "./useTabDrag";
import { WindowControls } from "./WindowControls";

export interface TabBarHandlers {
  onSelect(key: number): void;
  /** Opens a local terminal tab ("+"), when `canOpenLocal`. */
  onNew(): void;
  /** Closes these tabs, asking first if needed. */
  onClose(keys: number[]): void;
  /** Moves the tab to `index` in the tab order. */
  onMove(key: number, index: number): void;
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
  onToggleCompose(): void;
  onToggleQuickBar(): void;
}

interface Props {
  tabs: Tab[];
  activeKey: number | null;
  followRemoteTitle: boolean;
  /** Whether tabs in the background with new output are marked (a setting). */
  markOutput: boolean;
  /** Whether tabs in the background whose terminal rang its bell are marked (a setting). */
  bellAlert: boolean;
  /**
   * From `useStableHandlers`: a title changes several times a second in a busy terminal, and
   * the tab bar renders again each time, but the tabs whose title didn't change don't.
   */
  handlers: TabBarHandlers;
  /** Whether the system allows local terminals, which "+" opens. */
  canOpenLocal: boolean;
  /** The other windows, which a tab can move to. */
  windows: WindowInfo[];
  composeOpen: boolean;
  quickBarOpen: boolean;
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

export function TabBar({
  tabs,
  activeKey,
  followRemoteTitle,
  markOutput,
  bellAlert,
  handlers,
  canOpenLocal,
  windows,
  composeOpen,
  quickBarOpen,
  inScope,
  syncing,
  flashing,
  colorOf,
  addressOf,
}: Props) {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const {
    onSelect,
    onNew,
    onClose,
    onMove,
    canMoveToWindow,
    onMoveToWindow,
    onDropOut,
    onRename,
    onDuplicate,
    onSplit,
    canSplit,
    onSaveAsSession,
    onReconnect,
    onBreak,
    onLog,
    onShowLog,
    onTogglePanel,
    onToggleCompose,
    onToggleQuickBar,
  } = handlers;
  // Tabs that don't fit scroll sideways, without a scroll bar (see `.tab-strip`).
  const stripRef = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLElement>(null);
  // Where a tab dragged from another window would go, while it is over this tab bar: the
  // marker's left edge in the strip, measured when the drag moves rather than while rendering.
  const [dropLeft, setDropLeft] = useState<number | null>(null);
  useWindowEvent("tab-drag-over", (x) => {
    const strip = stripRef.current;
    if (x === null || !strip) return setDropLeft(null);
    const elements = [...strip.querySelectorAll<HTMLElement>(".tab")];
    const index = tabIndexAt(strip, x);
    const last = elements[elements.length - 1];
    setDropLeft(index < elements.length ? elements[index].offsetLeft : last ? last.offsetLeft + last.offsetWidth : 0);
  });
  const [editing, setEditing] = useState<number | null>(null);
  const [menu, setMenu] = useState<{ key: number; x: number; y: number } | null>(null);
  const tabsRef = useLatest(tabs);

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

  // Where the tab bar is, for tabs dragged from other windows (see `tab_drag/`): only when it
  // has moved, as both of these fire on each frame of a resize.
  useEffect(() => {
    const bar = barRef.current!;
    let last = "";
    const report = () => {
      const { x, y, width, height } = bar.getBoundingClientRect();
      const key = `${x} ${y} ${width} ${height}`;
      if (key === last) return;
      last = key;
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

  const { dragging, leaving, startDrag } = useTabDrag({ tabsRef, stripRef, onMove, canMoveToWindow, onDropOut });

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
  const onTabKeyDown = (e: ReactKeyboardEvent<HTMLElement>, key: number) => {
    if (editing !== null || isComposing(e)) return;
    const index = tabs.findIndex((tab) => tab.key === key);
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

  // The same from render to render, so that a tab renders again only when it changes.
  const actions = useStableHandlers<TabActions>({
    onKeyDown: onTabKeyDown,
    onMouseDown: (e, key) => editing !== key && startDrag(e, key),
    onSelect,
    onEdit: setEditing,
    onEdited: (key, value, title) => {
      setEditing(null);
      if (value !== null && value !== title) onRename(key, value || null);
    },
    onClose: (key) => onClose([key]),
    onMenu: (key, x, y) => setMenu({ key, x, y }),
  });

  /** Whether a pane of the tab has a question waiting in its file panel, out of sight. */
  const questionWaiting = (tab: Tab) =>
    tab.panes.some((p) => p.questions > 0 && !(tab.key === activeKey && tab.focused === p.key && tab.sidePanel === "files"));

  // Created here rather than in `segment`, so they stay the same from render to render.
  const toggleFiles = () => onTogglePanel("files");
  const toggleForwards = () => onTogglePanel("forwards");
  const segment = (panel: SidePanel, onToggle: () => void, label: string, hint: string, badge?: ReactNode) => (
    <button
      className={activeTab?.sidePanel === panel ? "on" : undefined}
      aria-pressed={activeTab?.sidePanel === panel}
      disabled={!activeTab || (unavailable && activeTab.sidePanel !== panel)}
      onClick={onToggle}
      title={unavailable ? t("tabs.panelUnavailable") : hint}
    >
      <span className="panel-switch-label">
        {label}
        {badge}
      </span>
    </button>
  );

  // The window's title bar: always shown, so "+" and room to move the window stay available.
  return (
    <nav className="tab-bar" ref={barRef} {...DRAG_REGION}>
      <div className="tab-strip" ref={stripRef} onWheel={onWheel} role="tablist" aria-label={t("tabs.listLabel")}>
        {tabs.map((tab) => (
          <TabItem
            key={tab.key}
            tab={tab}
            title={tabTitle(tab, followRemoteTitle)}
            active={tab.key === activeKey}
            marks={[
              tab.key === dragging && (leaving ? "dragging leaving" : "dragging"),
              inScope.includes(tab.key) && (syncing ? "in-scope syncing" : "in-scope"),
              flashing.includes(tab.key) && "flash",
            ]
              .filter(Boolean)
              .join(" ")}
            editing={editing === tab.key}
            color={colorOf(tab)}
            where={addressOf(tab)}
            newOutput={markOutput && tab.attention === "output"}
            bellRang={bellAlert && tab.attention === "bell"}
            questionWaiting={questionWaiting(tab)}
            actions={actions}
          />
        ))}
        {dropLeft !== null && <span className="tab-drop-marker" style={{ left: dropLeft }} aria-hidden="true" />}
      </div>
      {menu && tabs.some((tab) => tab.key === menu.key) && (
        <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.key)} onClose={() => setMenu(null)} />
      )}
      {canOpenLocal && (
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
        <div className="segmented panel-switch">
          {segment(
            "files",
            toggleFiles,
            t("tabs.files"),
            activeTab && questionWaiting(activeTab)
              ? t("tabs.filesHintQuestion", { shortcut: shortcutLabel("filePanel") })
              : t("tabs.filesHint", { shortcut: shortcutLabel("filePanel") }),
            activeTab && questionWaiting(activeTab) && <span className="badge question" />,
          )}
          {segment(
            "forwards",
            toggleForwards,
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

/** What a tab does, the same object for every tab and every render. */
interface TabActions {
  onKeyDown(e: ReactKeyboardEvent<HTMLElement>, key: number): void;
  onMouseDown(e: ReactMouseEvent, key: number): void;
  onSelect(key: number): void;
  onEdit(key: number): void;
  /** The title editor closed with `value` (null: cancelled) over `title`. */
  onEdited(key: number, value: string | null, title: string): void;
  onClose(key: number): void;
  onMenu(key: number, x: number, y: number): void;
}

interface TabItemProps {
  tab: Tab;
  title: string;
  active: boolean;
  /** Classes for the states the tab bar knows about: dragged, in the compose bar's scope… */
  marks: string;
  editing: boolean;
  /** Its session's color, if any. */
  color?: string;
  /** Where its session connects, for the tooltip. */
  where?: string;
  newOutput: boolean;
  bellRang: boolean;
  questionWaiting: boolean;
  actions: TabActions;
}

/**
 * One tab, its own component so that React Compiler memoizes it: the others don't render
 * again when a busy terminal changes its title, nor have their elements' handlers replaced.
 */
function TabItem(props: TabItemProps) {
  useStableProps("TabItem", props, ["actions"]);
  const { tab, title, active, marks, editing, color, where, newOutput, bellRang, questionWaiting, actions } = props;
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const { key } = tab;
  const pane = focusedPane(tab);

  const statusLabel = () => {
    if (pane.status === "connecting") return t("tabs.status.connecting");
    const parts = [
      pane.status === "closed" && t(pane.protocol === "local" ? "tabs.status.exited" : "tabs.status.closed"),
      bellRang && t("tabs.bell"),
      newOutput && t("tabs.newOutput"),
      questionWaiting && t("tabs.questionWaiting"),
    ].filter(Boolean);
    return parts.length > 0 ? parts.join(". ") : null;
  };
  const status = statusLabel();

  return (
    <div
      data-key={key}
      id={tabId(key)}
      role="tab"
      aria-selected={active}
      aria-controls={tabPanelId(key)}
      // The active tab is the one Tab reaches; arrows move to the others.
      tabIndex={active ? 0 : -1}
      onKeyDown={(e) => actions.onKeyDown(e, key)}
      className={["tab", active && "active", marks].filter(Boolean).join(" ")}
      onMouseDown={(e) => actions.onMouseDown(e, key)}
      onClick={() => actions.onSelect(key)}
      onDoubleClick={() => actions.onEdit(key)}
      onAuxClick={(e) => e.button === 1 && actions.onClose(key)}
      onContextMenu={(e) => {
        if (followsMenuKey(e)) return;
        e.preventDefault();
        actions.onMenu(key, e.clientX, e.clientY);
      }}
      title={editing ? undefined : [title, where, t("tabs.renameHint")].filter(Boolean).join("\n")}
    >
      {color && <span className="tab-color" style={{ background: color }} />}
      <span className={`status-dot ${pane.status}${newOutput ? " new-output" : ""}`} title={newOutput ? t("tabs.newOutput") : undefined} />
      {bellRang && (
        <span className="tab-bell" title={t("tabs.bell")}>
          <BellIcon />
        </span>
      )}
      {isLogging(pane) && <span className="tab-log" title={t("tabs.logging", { path: pane.logPath })} />}
      {questionWaiting && <span className="tab-question" title={t("tabs.questionWaiting")} />}
      {editing ? (
        <TitleEditor initial={title} onDone={(value) => actions.onEdited(key, value, title)} />
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
          actions.onClose(key);
        }}
      >
        <CloseIcon size={12} />
      </button>
    </div>
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
      autoCapitalize="off"
      autoCorrect="off"
    />
  );
}
