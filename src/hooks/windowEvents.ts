import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { listenHere } from "../ipc/events";
import type { Change } from "../ipc/bindings";

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
