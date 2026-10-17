import { useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { ContextMenu, type MenuItem } from "../components/ContextMenu";
import { menuLabel } from "../hooks/appMenu";
import { appMenu, matches, type MenuAction, type MenuEntry } from "../lib/keymap";
import { isMac } from "../lib/platform";
import { runAction, useShortcuts } from "../hooks/shortcuts";
import type { Release } from "../ipc/bindings";
import { useLocale } from "../hooks/locale";
import { useStableHandlers } from "../hooks/stableHandlers";

/** Where the menu opens when the button is out of sight (terminal full screen). */
const CORNER = 4;

/**
 * The button's ref and what it does: one object that stays the same (see `useStableHandlers`).
 * Whether the menu is open and the newer version come beside it, as values that change.
 */
export interface AppMenuButton {
  ref: RefObject<HTMLButtonElement | null>;
  /** Opens or closes the menu; `fromKeyboard` highlights its first item. */
  toggle(fromKeyboard: boolean): void;
}

/**
 * The app menu outside macOS (see `appMenu` in keymap.ts): opened by the app's icon and name in
 * the title bar (`button`), or by its shortcut, below the button or, while the button is out
 * of sight, in the window's corner. macOS shows the button too, the same menu, in full screen,
 * where the menu bar is out of sight; the menu has no shortcut there. Its items run their actions as their keys would
 * (`runAction`); `enabled` says which apply now, in this window. A newer version (`update`)
 * is marked on the way to "Check for Updates…"; the Microsoft Store package, which can't
 * check, has no such item.
 */
export function useAppMenu({
  enabled,
  fullScreen,
  updates,
}: {
  enabled(action: MenuAction): boolean;
  fullScreen: boolean;
  updates: { supported: boolean; available: Release | null };
}) {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const ref = useRef<HTMLButtonElement>(null);
  // `anchor` is the button, taken when the menu opens: a click on it toggles the menu rather than
  // closing it as a click outside.
  const [open, setOpen] = useState<{ x: number; y: number; fromKeyboard: boolean; anchor: HTMLButtonElement | null } | null>(null);

  const toggle = (fromKeyboard: boolean) => {
    if (open) {
      setOpen(null);
      return;
    }
    const anchor = ref.current;
    const rect = anchor?.checkVisibility() ? anchor.getBoundingClientRect() : null;
    setOpen(rect ? { x: rect.left, y: rect.bottom + 2, fromKeyboard, anchor } : { x: CORNER, y: CORNER, fromKeyboard, anchor });
  };

  useShortcuts((e) => {
    if (isMac || !matches(e, "appMenu")) return false;
    toggle(true);
    return true;
  });

  const badge = updates.available ? t("updates.available", { version: updates.available.version }) : undefined;
  const entries = (items: MenuEntry[]): MenuItem[] =>
    items
      .filter((action) => action !== "checkForUpdates" || updates.supported)
      .map((action) =>
        action === "separator"
          ? action
          : {
              label: action === "fullScreen" && fullScreen ? t("appMenu.exitFullScreen") : menuLabel(t, action),
              shortcut: shortcutLabel(action),
              disabled: !enabled(action),
              badge: action === "checkForUpdates" ? badge : undefined,
              onSelect: () => void runAction(action),
            },
      );

  // The sections as submenus, then the app's own items.
  const items = (): MenuItem[] =>
    appMenu(false).flatMap(({ section, items }): MenuItem[] =>
      section === "app"
        ? ["separator", ...entries(items)]
        : [{ label: t(`appMenu.${section}`), badge: section === "help" ? badge : undefined, submenu: entries(items) }],
    );

  const button = useStableHandlers<AppMenuButton>({ ref, toggle });
  const menu = open && (
    <ContextMenu
      x={open.x}
      y={open.y}
      items={items()}
      anchor={open.anchor}
      highlightFirst={open.fromKeyboard}
      onClose={() => setOpen(null)}
    />
  );
  return { button, open: open !== null, menu };
}
