import { useEffect, useEffectEvent } from "react";

import { isAppLocked } from "../lib/appLock";
import { isDialogOpen, type DialogHandle } from "./dialogs";
import { actionEvent, type Action } from "../lib/keymap";

/** Handles a key if it is one of its shortcuts, returning whether it did. */
export type ShortcutHandler = (e: KeyboardEvent) => boolean;

interface Options {
  /**
   * "tabs" (the default): shortcuts acting on the tabs, ignored while a dialog is open, since
   * they would move the focus to a terminal behind it; "always": also with a dialog open.
   */
  when?: "tabs" | "always";
  /** Shortcuts of a dialog, which apply only while it is the top one. */
  dialog?: DialogHandle;
  /** False to stop handling them for now (an inactive terminal). */
  enabled?: boolean;
}

interface Registration {
  handle: ShortcutHandler;
  applies(): boolean;
}

const registrations = new Set<Registration>();
let listening = false;

// Capture phase, so the terminal never sees an app shortcut. Keys typed while an input
// method composes are never shortcuts; `isComposing` alone, not `keyCode` 229 (see
// `isComposing` in platform.ts), which WebKit also gives ⌘, and other keys the input
// method sees while it is on but not composing. None apply while the app is locked.
function onKeyDown(e: KeyboardEvent) {
  if (e.isComposing || isAppLocked()) return;
  for (const registration of registrations) {
    if (registration.applies() && registration.handle(e)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
  }
}

/**
 * Runs `action`, chosen from a menu, as if one of its keys were pressed: the first handler
 * that applies now takes it. Returns whether one did.
 */
export function runAction(action: Action): boolean {
  if (isAppLocked()) return false;
  const e = actionEvent(action);
  for (const registration of registrations) if (registration.applies() && registration.handle(e)) return true;
  return false;
}

/**
 * Registers app shortcuts while mounted; all of them go through one listener. The handlers
 * also get the actions chosen from menus (`runAction`), which `matches` takes for their keys.
 */
export function useShortcuts(handle: ShortcutHandler, { when = "tabs", dialog, enabled = true }: Options = {}) {
  const onShortcut = useEffectEvent(handle);

  useEffect(() => {
    if (!enabled) return;
    if (!listening) {
      window.addEventListener("keydown", onKeyDown, true);
      listening = true;
    }
    const registration: Registration = {
      handle: (e) => onShortcut(e),
      applies: () => (dialog ? dialog.isTop() : when === "always" || !isDialogOpen()),
    };
    registrations.add(registration);
    return () => void registrations.delete(registration);
  }, [when, dialog, enabled]);
}
