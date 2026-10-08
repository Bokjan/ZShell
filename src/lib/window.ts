import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { isMac, isWindows } from "./platform";

/** Marks an element as part of the title bar that moves the window (only the element itself, not its children). */
export const DRAG_REGION = { "data-window-drag": "" };

/**
 * Whether a press on `target` at `y` is on the title bar: an empty part of it, or the strip
 * of a dialog's backdrop that covers it, so the window can still be moved while a dialog is
 * open.
 */
function onTitleBar(target: EventTarget | null, y: number) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.hasAttribute("data-window-drag")) return true;
  const height = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--titlebar-height"));
  return target.classList.contains("dialog-backdrop") && y < height;
}

/** Text fields, which keep the web view's menu (cut, copy, paste); the terminal's input is not one. */
function isTextField(target: EventTarget | null) {
  if (!(target instanceof HTMLElement) || target.closest(".xterm")) return false;
  if (target instanceof HTMLTextAreaElement || target.isContentEditable) return true;
  return target instanceof HTMLInputElement && !["checkbox", "radio", "button", "submit", "range", "color", "file"].includes(target.type);
}

/**
 * Keeps the web view's own context menu (Reload, Back, Inspect) out of the app: a right click
 * anywhere without a menu of the app's own shows nothing, except in text fields.
 */
export function suppressWebViewMenu() {
  window.addEventListener("contextmenu", (e) => {
    if (!isTextField(e.target)) e.preventDefault();
  });
}

const titleBarDoubleClick = () => invoke("window_title_double_click").catch(console.error);

/**
 * The title bar the app draws in place of the native one (see ARCHITECTURE.md): moving the
 * window from its empty parts, double-clicking them (on macOS as the system setting says,
 * so after the second press is released without moving, as natively), the system menu on a
 * right click on Windows, and the window state that styles it (`data-platform`,
 * `data-fullscreen` and `data-window-inactive` on the root element).
 */
export function useTitleBar() {
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.platform = isMac ? "mac" : isWindows ? "windows" : "other";
    const window_ = getCurrentWindow();

    // Pressed for a double click on macOS, which acts on release.
    let pressed: { x: number; y: number } | null = null;
    const onMouseDown = (e: MouseEvent) => {
      if (e.button !== 0 || !onTitleBar(e.target, e.clientY)) return;
      // No focus change, no text selection, and a backdrop doesn't close its dialog.
      e.preventDefault();
      e.stopPropagation();
      if (e.detail === 1) void window_.startDragging().catch(console.error);
      else if (e.detail === 2 && isMac) pressed = { x: e.clientX, y: e.clientY };
      else if (e.detail === 2) void titleBarDoubleClick();
    };
    const onMouseUp = (e: MouseEvent) => {
      if (e.button !== 0 || !pressed) return;
      const same = e.detail === 2 && e.clientX === pressed.x && e.clientY === pressed.y;
      pressed = null;
      if (same && onTitleBar(e.target, e.clientY)) void titleBarDoubleClick();
    };
    const onContextMenu = (e: MouseEvent) => {
      if (!onTitleBar(e.target, e.clientY)) return;
      e.preventDefault();
      e.stopPropagation();
      if (isWindows) void invoke("window_system_menu").catch(console.error);
    };
    // Capture phase: before the backdrop's own handler, which closes its dialog.
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("mouseup", onMouseUp, true);
    window.addEventListener("contextmenu", onContextMenu, true);

    // The traffic lights hide in full screen, and their space with them.
    const updateFullscreen = () =>
      window_
        .isFullscreen()
        .then((fullscreen) => {
          if (fullscreen) root.dataset.fullscreen = "";
          else delete root.dataset.fullscreen;
        })
        .catch(console.error);
    void updateFullscreen();
    const unlistenResize = window_.onResized(updateFullscreen);
    const unlistenFocus = window_.onFocusChanged(({ payload: focused }) => {
      if (focused) delete root.dataset.windowInactive;
      else root.dataset.windowInactive = "";
    });
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("mouseup", onMouseUp, true);
      window.removeEventListener("contextmenu", onContextMenu, true);
      void unlistenResize.then((f) => f());
      void unlistenFocus.then((f) => f());
    };
  }, []);
}
