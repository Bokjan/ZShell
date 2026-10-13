import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import type { WindowButton } from "../lib/bindings";
import { isWindows } from "../lib/platform";
import { listenHere, useWindowEvent } from "../lib/windows";
import { IconButton } from "./IconButton";
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from "./icons";

/** Glyphs of Segoe Fluent Icons (Windows 11), at the same code points in Segoe MDL2 Assets (Windows 10). */
const GLYPHS = { minimize: "", maximize: "", restore: "", close: "" };

/** The window buttons at the right end of the tab bar, on Windows and Linux. */
export function WindowControls() {
  return isWindows ? <WindowsControls /> : <LinuxControls />;
}

/** Whether the window is maximized, kept up to date. */
function useMaximized() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const window_ = getCurrentWindow();
    const update = () => window_.isMaximized().then(setMaximized).catch(console.error);
    void update();
    const unlisten = window_.onResized(update);
    return () => void unlisten.then((f) => f());
  }, []);
  return maximized;
}

/**
 * Minimize, maximize and close buttons for the undecorated window on Windows. A native window
 * over the maximize button (see `window/win.rs`) gives it the snap layouts; it takes the mouse
 * from the page, so it reports hover and press, and handles the click itself.
 */
function WindowsControls() {
  const { t } = useTranslation();
  const maximized = useMaximized();
  const [maximizeState, setMaximizeState] = useState({ hover: false, pressed: false });
  const maximizeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const unlisten = listenHere<{ hover: boolean; pressed: boolean }>("window-maximize-button", (e) => setMaximizeState(e.payload));
    return () => void unlisten.then((f) => f());
  }, []);

  // Keeps the native window over the button: it is right-aligned, so it moves with the
  // window's width, and the backend converts to device pixels with the current scale.
  useEffect(() => {
    const button = maximizeRef.current!;
    const place = () => {
      const r = button.getBoundingClientRect();
      invoke("window_set_maximize_button", { rect: { x: r.left, y: r.top, width: r.width, height: r.height } }).catch(console.error);
    };
    place();
    window.addEventListener("resize", place);
    const unlisten = getCurrentWindow().onScaleChanged(place);
    return () => {
      window.removeEventListener("resize", place);
      void unlisten.then((f) => f());
      invoke("window_set_maximize_button", { rect: null }).catch(console.error);
    };
  }, []);

  const window_ = getCurrentWindow();
  const maximizeClass = maximizeState.pressed ? "pressed" : maximizeState.hover ? "hover" : undefined;
  return (
    // The buttons don't take focus from the terminal.
    <div className="window-controls" onMouseDown={(e) => e.preventDefault()}>
      <IconButton tabIndex={-1} label={t("window.minimize")} onClick={() => void window_.minimize()}>
        {GLYPHS.minimize}
      </IconButton>
      <IconButton
        ref={maximizeRef}
        tabIndex={-1}
        className={maximizeClass}
        label={maximized ? t("window.restore") : t("window.maximize")}
        // Only reached if the native window over it is missing.
        onClick={() => void window_.toggleMaximize()}
      >
        {maximized ? GLYPHS.restore : GLYPHS.maximize}
      </IconButton>
      <IconButton tabIndex={-1} className="close" label={t("window.close")} onClick={() => void window_.close()}>
        {GLYPHS.close}
      </IconButton>
    </div>
  );
}

/**
 * The buttons the desktop puts on title bars (GNOME's button layout: only Close by default,
 * all three on Ubuntu), round as GTK draws them; GTK draws the rest of the window's frame.
 */
function LinuxControls() {
  const { t } = useTranslation();
  const maximized = useMaximized();
  const [buttons, setButtons] = useState<WindowButton[]>([]);
  const load = () => void invoke<WindowButton[]>("window_buttons").then(setButtons, console.error);
  useEffect(load, []);
  useWindowEvent("window-buttons-changed", load);

  const window_ = getCurrentWindow();
  if (buttons.length === 0) return null;
  return (
    // The buttons don't take focus from the terminal.
    <div className="window-controls" onMouseDown={(e) => e.preventDefault()}>
      {buttons.map((button) =>
        button === "minimize" ? (
          <IconButton key={button} tabIndex={-1} label={t("window.minimize")} onClick={() => void window_.minimize()}>
            <MinimizeIcon />
          </IconButton>
        ) : button === "maximize" ? (
          <IconButton
            key={button}
            tabIndex={-1}
            label={maximized ? t("window.restore") : t("window.maximize")}
            onClick={() => void window_.toggleMaximize()}
          >
            {maximized ? <RestoreIcon /> : <MaximizeIcon />}
          </IconButton>
        ) : (
          <IconButton key={button} tabIndex={-1} className="close" label={t("window.close")} onClick={() => void window_.close()}>
            <CloseIcon />
          </IconButton>
        ),
      )}
    </div>
  );
}
