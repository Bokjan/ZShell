import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

import type { Change, WindowInfo } from "./bindings";
import type { TabTransfer } from "./tabs";

export type { Change, WindowInfo } from "./bindings";

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

/** Calls `handler` when another window saved what this one shows (see `Change`). */
export function useConfigChanged(change: Change, handler: () => void) {
  useWindowEvent<Change>("config-changed", (changed) => changed === change && handler());
}

/** What a new window shows first: a tab moved out of another window, or a saved session to open. */
export type WindowContent = { kind: "tab"; tab: TabTransfer } | { kind: "profile"; profileId: string; title: string };

export const windows = {
  /** Opens a window next to this one, showing `content`. */
  open: (content: WindowContent) => invoke<void>("window_open", { content }),
  /** What this window was opened to show; once, and null for the first window. */
  handoff: () => invoke<WindowContent | null>("window_handoff"),
  /** The other windows, in the order they opened. */
  list: () => invoke<WindowInfo[]>("window_list"),
  setTitle: (title: string) => invoke<void>("window_set_title", { title }),
  /** Gives a tab to another window, which comes forward (`window.notFound` if it has closed). */
  sendTab: (target: string, tab: TabTransfer) => invoke<void>("window_send_tab", { target, tab }),
  /** How many tabs in all the windows are busy (connected, running a program), for quitting. */
  busyCount: () => invoke<number>("app_busy_count"),
  /** This window's answer to the `busy-query` event. */
  busyReply: (query: number, count: number) => invoke<void>("window_busy_reply", { query, count }),
  quit: () => invoke<void>("app_quit"),
};
