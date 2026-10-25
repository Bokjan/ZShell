import type { Terminal } from "@xterm/xterm";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

import type { TerminalSettings } from "../../ipc/bindings";
import { attemptSync } from "../../lib/attempt";
import { Disposables, type Disposable } from "../../lib/disposables";
import { clipboardKeyOf } from "../../lib/keymap";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../../lib/menuKey";
import type { PasteTarget } from "../../lib/sessionRegistry";

const ignore = () => {};

// Through the clipboard plugin: pasting from a menu can't raise the browser's paste event,
// and WKWebView's `navigator.clipboard.readText` asks the user with a system "Paste" button.
export const copyText = (text: string) => void writeText(text).catch(console.error);

/** Pastes the clipboard's text with `paste`. */
export const pasteClipboard = (paste: (text: string) => void) => void readText().then(paste, ignore);

/** Where the context menu opens; `link`: the link it was opened on. */
export interface MenuAt {
  x: number;
  y: number;
  link?: string;
}

export interface ClipboardDeps {
  /** Whether there is no session to paste into. */
  closed(): boolean;
  /** The other panes a paste goes to as well (syncing). */
  pasteTargets(): PasteTarget[];
  settings(): TerminalSettings;
  /** Asks before pasting multi-line text, which then goes through `pasteAll`. */
  confirm(text: string): void;
  openMenu(at: MenuAt): void;
  /** The link under the pointer, for the context menu. */
  link(): string | null;
}

/** Pasting into the terminal; `pasting` while a paste goes into it, which isn't typed input. */
export interface Clipboard extends Disposable {
  /** Pastes text, asking first if it would run several commands. */
  paste(text: string): void;
  /** Pastes into this pane and the panes synced with it, without asking. */
  pasteAll(text: string): void;
  pasting(): boolean;
}

/**
 * The terminal's clipboard: pasting (from the keys, ⌘V and the browser, and the right button
 * in "paste" mode), copying (from the keys, and on select), and the context menu's keys and
 * click. All pastes go through `term.paste()`.
 */
export function attachClipboard(term: Terminal, container: HTMLElement, deps: ClipboardDeps): Clipboard {
  const held = new Disposables();
  // While syncing, a paste isn't passed on as typed input (wrapped for bracketed paste, or
  // not, as this terminal is): each synced pane pastes it as its own terminal requires.
  let pasting = false;
  const pasteAll = (text: string) => {
    const others = deps.pasteTargets();
    pasting = true;
    attemptSync(
      () => term.paste(text),
      () => (pasting = false),
    );
    for (const other of others) other.paste(text);
  };
  const paste = (text: string) => {
    // Nowhere to go; a line break in it would otherwise read as Enter, which reconnects.
    if (!text || deps.closed()) return;
    // With bracketed paste the shell inserts the lines without running them: asked unless
    // every pane it goes to does.
    const bracketed = term.modes.bracketedPasteMode && deps.pasteTargets().every((other) => other.bracketed());
    const confirm = deps.settings().confirmMultilinePaste && !bracketed;
    if (confirm && /[\r\n]/.test(text)) deps.confirm(text);
    else pasteAll(text);
  };

  // Ctrl+Shift+C / Ctrl+V and friends outside macOS (see `clipboardKeyOf`).
  term.attachCustomKeyEventHandler((e) => {
    // The menu key and Shift+F10 open the context menu, at the cursor (where xterm.js keeps
    // its text field), whatever the program does with the mouse and right-clicks do. The
    // browser may fire `contextmenu` for them as well, which can't tell them from a click.
    if (isMenuKey(e)) {
      if (e.type === "keydown") {
        e.preventDefault();
        openedByMenuKey();
        const at = term.textarea?.getBoundingClientRect() ?? container.getBoundingClientRect();
        deps.openMenu({ x: at.left, y: at.bottom });
      }
      return false;
    }
    const key = clipboardKeyOf(e);
    if (!key || (key === "copyIfSelected" && !term.hasSelection())) return true;
    if (e.type === "keydown") {
      e.preventDefault();
      if (key === "paste") pasteClipboard(paste);
      else {
        copyText(term.getSelection());
        // So that the next Ctrl+C interrupts again.
        term.clearSelection();
      }
    }
    return false;
  });

  // ⌘V (the native Edit menu), and pastes from the browser's own key handling.
  held.listen(
    container,
    "paste",
    (e: ClipboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      paste(e.clipboardData?.getData("text/plain") ?? "");
    },
    true,
  );

  // Copy on select: once a mouse selection is finished, wherever the button is released.
  let selecting = false;
  held.listen(
    container,
    "mousedown",
    (e: MouseEvent) => {
      selecting = e.button === 0;
    },
    true,
  );
  held.listen(window, "mouseup", () => {
    if (!selecting) return;
    selecting = false;
    if (deps.settings().copyOnSelect && term.hasSelection()) copyText(term.getSelection());
  });

  // After xterm.js's own handler (which may select the word under the pointer).
  held.listen(container, "contextmenu", (e: MouseEvent) => {
    if (followsMenuKey(e)) return;
    e.preventDefault();
    // Programs using the mouse get the click; Shift+right-click still opens the menu.
    if (term.modes.mouseTrackingMode !== "none" && !e.shiftKey) return;
    if (deps.settings().rightClick === "paste" && !e.shiftKey) {
      pasteClipboard(paste);
      return;
    }
    deps.openMenu({ x: e.clientX, y: e.clientY, link: deps.link() ?? undefined });
  });

  return { paste, pasteAll, pasting: () => pasting, dispose: () => held.dispose() };
}
