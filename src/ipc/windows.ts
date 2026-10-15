import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";

import { call } from "./call";
import type { DropTarget, Dropped, Rect, WindowButton, WindowInfo } from "./bindings";
import { transferSessions, type TabTransfer } from "../lib/tabs";

export type { Change, DropTarget, WindowInfo } from "./bindings";

/**
 * What a window is to show: a tab moved out of another window (dropped `at` that many CSS
 * pixels from the left of the page, if dragged there), or a saved session to open.
 */
export type WindowContent = { kind: "tab"; tab: TabTransfer; at?: number | null } | { kind: "profile"; profileId: string; title: string };

export const windows = {
  /** Where this window's maximize button is (Windows, which snaps windows to it), or none. */
  setMaximizeButton: (rect: Rect | null) => void call("window_set_maximize_button", { rect }).catch(console.error),
  /** The window buttons the page draws, in order (Windows and Linux). */
  buttons: () => call<WindowButton[]>("window_buttons"),
  /** Where files can be dragged out of the window, or nowhere (see `window_set_drag_area`). */
  setDragArea: (rect: Rect | null) => void call("window_set_drag_area", { rect }).catch(console.error),
  /** The native window's appearance; null follows the system. */
  setTheme: (theme: "dark" | "light" | null) => void call("window_set_theme", { theme }).catch(console.error),
  /** The color of the border of a snapped window (Windows). */
  setSnappedBorderColor: (color: string) => void call("window_set_snapped_border_color", { color }).catch(console.error),
  /** What the system does on a double click in the title bar (zoom, minimize). */
  titleDoubleClick: () => void call("window_title_double_click").catch(console.error),
  /** The system's window menu (Windows and Linux). */
  systemMenu: () => void call("window_system_menu").catch(console.error),
  /** Asks the system to draw attention to this window (the Dock icon bounces, the taskbar button flashes). */
  requestAttention: () => void getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(console.error),
  /**
   * Opens a window showing `content`, and returns its label: next to this one, or where a tab
   * was `dropped` outside the windows.
   */
  open: (content: WindowContent, dropped?: Dropped) =>
    call<string>("window_open", {
      content,
      sessions: content.kind === "tab" ? transferSessions(content.tab) : [],
      dropped: dropped ?? null,
    }),
  /**
   * What this window is to show and hasn't taken yet: what it opened for, and the tabs other
   * windows gave it (`tab-received` says when). Each is taken once.
   */
  take: () => call<WindowContent[]>("window_take"),
  /**
   * Closes this window, which has no tabs left, unless it is the last one or a tab is waiting
   * for it to take; returns whether it closes.
   */
  closeEmpty: () => call<boolean>("window_close_empty"),
  /** The other windows, in the order they opened. */
  list: () => call<WindowInfo[]>("window_list"),
  setTitle: (title: string) => call<void>("window_set_title", { title }),
  /**
   * Gives a tab to another window, which comes forward (`window.notFound` if it has closed)
   * and may take the tab's sessions from this one.
   */
  sendTab: (target: string, tab: TabTransfer, at?: number) =>
    call<void>("window_send_tab", { target, tab, sessions: transferSessions(tab), at: at ?? null }),
  /** Tells the window a tab came from that its pane `pane` couldn't move here (`pane-stayed`). */
  paneStayed: (target: string, pane: number) => call<void>("window_pane_stayed", { target, pane }),
  /** How many tabs in all the windows are busy (connected, running a program), for quitting. */
  busyCount: () => call<number>("app_busy_count"),
  /** This window's answer to the `busy-query` event. */
  busyReply: (query: number, count: number) => call<void>("window_busy_reply", { query, count }),
  quit: () => call<void>("app_quit"),
};

/** Dragging a tab out of its window (see `tab_drag/`). */
export const tabDrag = {
  /** Where this window's tab bar is, for tabs dragged from the other windows. */
  setTabBar: (rect: Rect) => call<void>("window_set_tab_bar", { rect }),
  /**
   * Where the tab of drag `id` from this window would go now; the window it would go into
   * shows where (`tab-drag-over`) while the drag goes on. `outside`: the pointer is outside
   * this page, which is all the backend goes by where it can't read the pointer (Wayland).
   */
  move: (id: string, outside: boolean) => call<DropTarget>("tab_drag_move", { id, outside }),
  /** Ends drag `id`: where the tab goes if `dropped`, or null. */
  end: (id: string, dropped: boolean, outside: boolean) => call<DropTarget | null>("tab_drag_end", { id, dropped, outside }),
};
