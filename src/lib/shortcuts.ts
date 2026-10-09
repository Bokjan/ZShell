import { useEffect, useRef } from "react";

import { isDialogOpen, type DialogHandle } from "./dialogs";

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
// method sees while it is on but not composing.
function onKeyDown(e: KeyboardEvent) {
  if (e.isComposing) return;
  for (const registration of registrations) {
    if (registration.applies() && registration.handle(e)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
  }
}

/** Registers app shortcuts while mounted; all of them go through one listener. */
export function useShortcuts(handle: ShortcutHandler, { when = "tabs", dialog, enabled = true }: Options = {}) {
  const latest = useRef(handle);
  latest.current = handle;

  useEffect(() => {
    if (!enabled) return;
    if (!listening) {
      window.addEventListener("keydown", onKeyDown, true);
      listening = true;
    }
    const registration: Registration = {
      handle: (e) => latest.current(e),
      applies: () => (dialog ? dialog.isTop() : when === "always" || !isDialogOpen()),
    };
    registrations.add(registration);
    return () => void registrations.delete(registration);
  }, [when, dialog, enabled]);
}
