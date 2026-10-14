import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { Change, DropTarget, Dropped, Rect, WindowInfo } from "./bindings";
import { transferSessions, type TabTransfer } from "./tabs";

export type { Change, DropTarget, WindowInfo } from "./bindings";

/**
 * Listens to an event for this window: sent to it, or to all windows. (The event module's
 * `listen` also hears what is sent to the other windows.)
 */
export const listenHere = <T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen<T>(event, handler);

/** Listens to an event for this window while the component is mounted; `handler` may change. */
export function useWindowEvent<T>(event: string, handler: (payload: T) => void) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  useEffect(() => {
    const unlisten = listenHere<T>(event, (e) => handlerRef.current(e.payload));
    return () => void unlisten.then((f) => f());
  }, [event]);
}

/**
 * Calls `handler` each time this window gets the focus back, while the component is mounted:
 * to read again what can change while the user is elsewhere (another window, another app).
 */
export function useWindowFocus(handler: () => void) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  useEffect(() => {
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => focused && handlerRef.current());
    return () => void unlisten.then((f) => f());
  }, []);
}

/** Calls `handler` when another window saved what this one shows (see `Change`). */
export function useConfigChanged(change: Change, handler: () => void) {
  useWindowEvent<Change>("config-changed", (changed) => changed === change && handler());
}

/**
 * What a window is to show: a tab moved out of another window (dropped `at` that many CSS
 * pixels from the left of the page, if dragged there), or a saved session to open.
 */
export type WindowContent = { kind: "tab"; tab: TabTransfer; at?: number | null } | { kind: "profile"; profileId: string; title: string };

export const windows = {
  /**
   * Opens a window showing `content`, and returns its label: next to this one, or where a tab
   * was `dropped` outside the windows.
   */
  open: (content: WindowContent, dropped?: Dropped) =>
    invoke<string>("window_open", {
      content,
      sessions: content.kind === "tab" ? transferSessions(content.tab) : [],
      dropped: dropped ?? null,
    }),
  /**
   * What this window is to show and hasn't taken yet: what it opened for, and the tabs other
   * windows gave it (`tab-received` says when). Each is taken once.
   */
  take: () => invoke<WindowContent[]>("window_take"),
  /**
   * Closes this window, which has no tabs left, unless it is the last one or a tab is waiting
   * for it to take; returns whether it closes.
   */
  closeEmpty: () => invoke<boolean>("window_close_empty"),
  /** The other windows, in the order they opened. */
  list: () => invoke<WindowInfo[]>("window_list"),
  setTitle: (title: string) => invoke<void>("window_set_title", { title }),
  /**
   * Gives a tab to another window, which comes forward (`window.notFound` if it has closed)
   * and may take the tab's sessions from this one.
   */
  sendTab: (target: string, tab: TabTransfer, at?: number) =>
    invoke<void>("window_send_tab", { target, tab, sessions: transferSessions(tab), at: at ?? null }),
  /** Tells the window a tab came from that its pane `pane` couldn't move here (`pane-stayed`). */
  paneStayed: (target: string, pane: number) => invoke<void>("window_pane_stayed", { target, pane }),
  /** How many tabs in all the windows are busy (connected, running a program), for quitting. */
  busyCount: () => invoke<number>("app_busy_count"),
  /** This window's answer to the `busy-query` event. */
  busyReply: (query: number, count: number) => invoke<void>("window_busy_reply", { query, count }),
  quit: () => invoke<void>("app_quit"),
};

/** Dragging a tab out of its window (see `tab_drag/`). */
export const tabDrag = {
  /** Where this window's tab bar is, for tabs dragged from the other windows. */
  setTabBar: (rect: Rect) => invoke<void>("window_set_tab_bar", { rect }),
  /**
   * Where the tab of drag `id` from this window would go now; the window it would go into
   * shows where (`tab-drag-over`) while the drag goes on. `outside`: the pointer is outside
   * this page, which is all the backend goes by where it can't read the pointer (Wayland).
   */
  move: (id: string, outside: boolean) => invoke<DropTarget>("tab_drag_move", { id, outside }),
  /** Ends drag `id`: where the tab goes if `dropped`, or null. */
  end: (id: string, dropped: boolean, outside: boolean) => invoke<DropTarget | null>("tab_drag_end", { id, dropped, outside }),
};
