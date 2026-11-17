import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { LocalShell, Profile } from "../ipc/api";
import { loadLocalShells, useLocalShells } from "../hooks/localShells";
import { useDirectory } from "../hooks/stores";
import { foundName, localOptions } from "../lib/localShells";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { IconButton } from "./IconButton";
import { ChevronIcon, PlusIcon } from "./icons";

interface Props {
  /** The label of "+", with its shortcut. */
  label: string;
  /** Opens the default local terminal. */
  onNew(): void;
  onOpenShell(shell: LocalShell): void;
  onOpenProfile(profile: Profile): void;
}

/**
 * "+" in the tab bar, split: the button opens the default local terminal, the arrow beside it a
 * menu of the shells found here and the saved local sessions. Its own component, so that the
 * tab bar doesn't render again as the sessions change.
 */
export function NewLocalButton({ label, onNew, onOpenShell, onOpenProfile }: Props) {
  const { t } = useTranslation();
  const found = useLocalShells();
  const { profiles } = useDirectory();
  const arrowRef = useRef<HTMLButtonElement>(null);
  // `anchor` is the arrow, taken when the menu opens (see `ContextMenu`).
  const [menu, setMenu] = useState<{ x: number; y: number; fromKeyboard: boolean; anchor: HTMLButtonElement } | null>(null);

  const toggleMenu = (fromKeyboard: boolean) => {
    const anchor = arrowRef.current;
    const rect = anchor?.getBoundingClientRect();
    if (menu || !anchor || !rect) return setMenu(null);
    // Another window may have found them again since.
    void loadLocalShells();
    setMenu({ x: rect.left, y: rect.bottom + 2, fromKeyboard, anchor });
  };

  const sessions = profiles.filter((profile) => localOptions(profile));
  const shells: MenuItem[] =
    found === null
      ? [{ label: t("tabs.findingShells"), disabled: true, onSelect: () => {} }]
      : found.map((shell) => ({ label: foundName(shell, t), onSelect: () => onOpenShell(shell.shell) }));
  const items: MenuItem[] = [
    ...shells,
    ...(sessions.length > 0 ? ["separator" as const] : []),
    ...sessions.map((profile) => ({ label: profile.name, onSelect: () => onOpenProfile(profile) })),
  ];

  return (
    <>
      <IconButton className="tab-new" label={label} onMouseDown={(e) => e.preventDefault()} onClick={onNew}>
        <PlusIcon />
      </IconButton>
      <IconButton
        ref={arrowRef}
        className="tab-new-menu"
        label={t("tabs.newLocalMenu")}
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        onMouseDown={(e) => e.preventDefault()}
        // Opened from the keyboard (Enter, Space) when the click has no pointer position.
        onClick={(e) => toggleMenu(e.detail === 0)}
        onKeyDown={(e) => {
          if (e.key !== "ArrowDown" || menu) return;
          e.preventDefault();
          toggleMenu(true);
        }}
      >
        <ChevronIcon inline={false} />
      </IconButton>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={items.length > 0 ? items : [{ label: t("tabs.noShells"), disabled: true, onSelect: () => {} }]}
          anchor={menu.anchor}
          highlightFirst={menu.fromKeyboard}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}
