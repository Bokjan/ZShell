import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

import type { Events } from "./bindings";

export type { Events } from "./bindings";

/**
 * Listens to a backend event for this window: sent to it, or to all windows. (The event
 * module's `listen` also hears what is sent to the other windows.) The payload's type comes
 * with the event's name (`Events`, generated from `events.rs`).
 */
export const listenHere = <E extends keyof Events>(event: E, handler: EventCallback<Events[E]>): Promise<UnlistenFn> =>
  getCurrentWebviewWindow().listen<Events[E]>(event, handler);
