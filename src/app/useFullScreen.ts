import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** How long the hint with the keys that leave full screen stays. */
const HINT_MS = 3000;

/**
 * Full screen for the terminals: the window in full screen showing only the active tab's
 * panes, without the session list, the tabs, the bars and the side panel, which are hidden
 * rather than unmounted so that they come back as they were. Leaving the window's full
 * screen another way (the green button, View › Exit Full Screen) or closing the last tab
 * ends it too; a window that was already in full screen stays so.
 */
export function useFullScreen(hasTabs: boolean, onChange: (on: boolean) => void) {
  const [on, setOn] = useState(false);
  const [hint, setHint] = useState(false);
  // The window was in full screen before (macOS ⌃⌘F), so leaving keeps it there.
  const wasFullScreen = useRef(false);
  const latest = useRef(onChange);
  latest.current = onChange;

  const change = (next: boolean) => {
    setOn(next);
    setHint(next);
    latest.current(next);
  };

  const enter = async () => {
    const window_ = getCurrentWindow();
    wasFullScreen.current = await window_.isFullscreen();
    if (!wasFullScreen.current) await window_.setFullscreen(true);
    change(true);
  };

  const exit = async () => {
    change(false);
    if (!wasFullScreen.current) await getCurrentWindow().setFullscreen(false);
  };

  const toggle = () => void (on ? exit() : enter()).catch(console.error);

  useEffect(() => {
    if (!hint) return;
    const timer = setTimeout(() => setHint(false), HINT_MS);
    return () => clearTimeout(timer);
  }, [hint]);

  // The window leaving full screen by itself. On macOS it reaches full screen only once its
  // animation has started, so a size change before that doesn't count.
  useEffect(() => {
    if (!on) return;
    const window_ = getCurrentWindow();
    let reached = false;
    const unlisten = window_.onResized(() =>
      window_
        .isFullscreen()
        .then((fullScreen) => {
          if (fullScreen) reached = true;
          else if (reached || wasFullScreen.current) change(false);
        })
        .catch(console.error),
    );
    return () => void unlisten.then((f) => f());
  }, [on]);

  useEffect(() => {
    if (on && !hasTabs) void exit().catch(console.error);
  }, [on, hasTabs]);

  return { on, hint, toggle };
}
