import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/**
 * Listens to an event for this window: sent to it, or to all windows. (The event module's
 * `listen` also hears what is sent to the other windows.)
 */
export const listenHere = <T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen<T>(event, handler);
