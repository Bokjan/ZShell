import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import type { NativeMenuSection } from "./bindings";
import { appMenu, macKeys, setNativeMenu, type MenuAction } from "./keymap";
import { isMac } from "./platform";
import { runAction } from "./shortcuts";
import { useWindowEvent, useWindowFocus } from "./windows";

export const RELEASES_URL = "https://github.com/Bokjan/ZShell/releases";
export const ISSUES_URL = "https://github.com/Bokjan/ZShell/issues";

export const menuLabel = (t: TFunction, action: MenuAction) => t(`appMenu.items.${action}`);

/**
 * The app menu on macOS: has the backend build the menu bar from `appMenu` (see menu.rs), and
 * runs the actions chosen there in this window, as if their keys were pressed. Used for the
 * whole page, so that the unlock screen has them too.
 */
export function useNativeMenu() {
  const { t } = useTranslation();

  useEffect(() => {
    if (!isMac) return;
    const sections: NativeMenuSection[] = appMenu(true).map(({ section, items }) => ({
      section,
      items: items.map((item) =>
        item === "separator" ? { kind: "separator" } : { kind: "action", id: item, label: menuLabel(t, item), keys: macKeys(item) },
      ),
    }));
    invoke("app_menu_set", { sections }).then(setNativeMenu, console.error);
  }, [t]);

  useWindowEvent<MenuAction>("menu-action", (action) => void runAction(action));
}

/**
 * On macOS, the label of an item of the menu bar that tells this window's state (terminal
 * full screen): the menu acts on the window that last had the focus, so each window sets it
 * as its state changes and when it gets the focus.
 */
export function useNativeMenuLabel(action: MenuAction, label: string) {
  const show = () => {
    if (isMac) invoke("app_menu_set_label", { id: action, label }).catch(console.error);
  };
  useEffect(show, [action, label]);
  useWindowFocus(show);
}
