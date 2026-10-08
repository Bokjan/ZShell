import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

/** Glyphs of Segoe Fluent Icons (Windows 11), at the same code points in Segoe MDL2 Assets (Windows 10). */
const GLYPHS = { minimize: "", maximize: "", restore: "", close: "" };

/**
 * Minimize, maximize and close buttons for the undecorated window on Windows, at the right
 * end of the tab bar. A native window over the maximize button (see `window.rs`) gives it the
 * snap layouts; it takes the mouse from the page, so it reports hover and press, and handles
 * the click itself.
 */
export function WindowControls() {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const [maximizeState, setMaximizeState] = useState({ hover: false, pressed: false });
  const maximizeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const window_ = getCurrentWindow();
    const update = () => window_.isMaximized().then(setMaximized).catch(console.error);
    void update();
    const unlisten = window_.onResized(update);
    return () => void unlisten.then((f) => f());
  }, []);

  useEffect(() => {
    const unlisten = listen<{ hover: boolean; pressed: boolean }>("window-maximize-button", (e) =>
      setMaximizeState(e.payload),
    );
    return () => void unlisten.then((f) => f());
  }, []);

  // Keeps the native window over the button: it is right-aligned, so it moves with the
  // window's width, and the backend converts to device pixels with the current scale.
  useEffect(() => {
    const button = maximizeRef.current!;
    const place = () => {
      const r = button.getBoundingClientRect();
      invoke("window_set_maximize_button", { rect: { x: r.left, y: r.top, width: r.width, height: r.height } }).catch(
        console.error,
      );
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
      <button tabIndex={-1} title={t("window.minimize")} onClick={() => void window_.minimize()}>
        {GLYPHS.minimize}
      </button>
      <button
        ref={maximizeRef}
        tabIndex={-1}
        className={maximizeClass}
        title={maximized ? t("window.restore") : t("window.maximize")}
        // Only reached if the native window over it is missing.
        onClick={() => void window_.toggleMaximize()}
      >
        {maximized ? GLYPHS.restore : GLYPHS.maximize}
      </button>
      <button tabIndex={-1} className="close" title={t("window.close")} onClick={() => void window_.close()}>
        {GLYPHS.close}
      </button>
    </div>
  );
}
