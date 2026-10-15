import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { listenHere, type Events } from "../ipc/events";
import type { Change } from "../ipc/bindings";

/** Listens to an event for this window while the component is mounted; `handler` may change. */
export function useWindowEvent<E extends keyof Events>(event: E, handler: (payload: Events[E]) => void) {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  useEffect(() => {
    const unlisten = listenHere(event, (e) => handlerRef.current(e.payload));
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
  useWindowEvent("config-changed", (changed) => changed === change && handler());
}
