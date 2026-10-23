import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useLatest } from "../hooks/latest";
import { useLocale } from "../hooks/locale";
import { announce } from "../lib/announce";
import { storedString, storeString } from "../lib/storage";

const HIDDEN_KEY = "zshell.sidebarHidden";

/**
 * Whether the session list is hidden, the rest of the window staying as it is (the tabs, the
 * side panel, the status bar); its title bar part then goes to the start of the tab bar (see
 * `Sidebar`). Layout state like the list's width, so kept per machine rather than in the
 * settings: the window it changed in keeps it, and new windows and the next start take the
 * last one.
 *
 * Hiding it while it has the focus gives the focus to the terminal (`focusTerminal`), unless
 * it was on its button, which `Sidebar` focuses again where it is now.
 */
export function useSidebar(focusTerminal: () => void) {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const [hidden, setHidden] = useState(() => storedString(HIDDEN_KEY) === "true");
  const hiddenRef = useLatest(hidden);

  const set = (next: boolean) => {
    if (next === hiddenRef.current) return;
    const focused = document.activeElement;
    const inList = !!focused?.closest(".sidebar") && !focused.closest(".sidebar-toggle");
    setHidden(next);
    storeString(HIDDEN_KEY, String(next));
    if (next && inList) focusTerminal();
  };

  const toggle = () => {
    const next = !hiddenRef.current;
    set(next);
    announce(next ? t("announce.sidebarHidden", { shortcut: shortcutLabel("sidebar") }) : t("announce.sidebarShown"));
  };

  /** Shows it for its search box, which takes the focus and says where it is. */
  const show = () => set(false);

  return { hidden, toggle, show };
}
