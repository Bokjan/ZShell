import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ContextMenu, type MenuItem } from "../components/ContextMenu";
import type { AppMenuButton } from "../components/Sidebar";
import { menuLabel } from "../hooks/appMenu";
import { appMenu, matches, type MenuAction, type MenuEntry } from "../lib/keymap";
import { isMac } from "../lib/platform";
import { runAction, useShortcuts } from "../hooks/shortcuts";
import { useLocale } from "../hooks/locale";
import { useStableHandlers } from "../hooks/stableHandlers";

/** Where the menu opens when the button is out of sight (terminal full screen). */
const CORNER = 4;

/**
 * The button's ref and what it does: one object that stays the same (see `useStableHandlers`).
 * Whether the menu is open comes beside it, as a value that changes.
 */
/**
 * The app menu outside macOS (see `appMenu` in keymap.ts): opened by the app's icon and name in
 * the title bar (`button`), or by its shortcut, below the button or, while the button is out
 * of sight, in the window's corner. macOS shows the button too, the same menu, in full screen,
 * where the menu bar is out of sight; the menu has no shortcut there. Its items run their actions as their keys would
 * (`runAction`); `enabled` says which apply now, in this window. The Microsoft Store package,
 * which can't check for a newer version, has no "Check for Updates…".
 */
export function useAppMenu({
  enabled,
  fullScreen,
  sidebarHidden,
  updates,
}: {
  enabled(action: MenuAction): boolean;
  fullScreen: boolean;
  sidebarHidden: boolean;
  updates: { supported: boolean };
}) {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const ref = useRef<HTMLButtonElement>(null);
  // `anchor` is the button, taken when the menu opens: a click on it toggles the menu rather than
  // closing it as a click outside. The items are those that apply as it opens (see
  // "渲染与 memo" in ARCHITECTURE.md): what decides it isn't state of this component.
  const [open, setOpen] = useState<{
    x: number;
    y: number;
    fromKeyboard: boolean;
    anchor: HTMLButtonElement | null;
    items: MenuItem[];
  } | null>(null);

  // Items that tell the window's state.
  const label = (action: MenuAction) =>
    action === "fullScreen" && fullScreen
      ? t("appMenu.exitFullScreen")
      : action === "sidebar" && sidebarHidden
        ? t("appMenu.showSidebar")
        : menuLabel(t, action);
  const entries = (items: MenuEntry[]): MenuItem[] =>
    items
      .filter((action) => action !== "checkForUpdates" || updates.supported)
      .map((action) =>
        action === "separator"
          ? action
          : {
              label: label(action),
              shortcut: shortcutLabel(action),
              disabled: !enabled(action),
              onSelect: () => void runAction(action),
            },
      );

  // The sections as submenus, then the app's own items.
  const items = (): MenuItem[] =>
    appMenu(false).flatMap(({ section, items }): MenuItem[] =>
      section === "app" ? ["separator", ...entries(items)] : [{ label: t(`appMenu.${section}`), submenu: entries(items) }],
    );

  const toggle = (fromKeyboard: boolean) => {
    if (open) {
      setOpen(null);
      return;
    }
    const anchor = ref.current;
    const rect = anchor?.checkVisibility() ? anchor.getBoundingClientRect() : null;
    const at = rect ? { x: rect.left, y: rect.bottom + 2 } : { x: CORNER, y: CORNER };
    setOpen({ ...at, fromKeyboard, anchor, items: items() });
  };

  useShortcuts((e) => {
    if (isMac || !matches(e, "appMenu")) return false;
    toggle(true);
    return true;
  });

  const button = useStableHandlers<AppMenuButton>({ ref, toggle });
  const menu = open && (
    <ContextMenu
      x={open.x}
      y={open.y}
      items={open.items}
      anchor={open.anchor}
      highlightFirst={open.fromKeyboard}
      onClose={() => setOpen(null)}
    />
  );
  return { button, open: open !== null, menu };
}
