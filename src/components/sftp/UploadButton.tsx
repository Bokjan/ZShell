import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ContextMenu } from "../ContextMenu";
import { ChevronIcon } from "../icons";

/**
 * Uploading files or a folder, from the menu of the Upload button: one button rather than two
 * keeps the row on one line in languages with longer words.
 */
export function UploadButton({ disabled, onUpload }: { disabled: boolean; onUpload(directory: boolean): void }) {
  const { t } = useTranslation();
  const buttonRef = useRef<HTMLButtonElement>(null);
  // `anchor` is the button, taken when the menu opens (see `ContextMenu`).
  const [menu, setMenu] = useState<{ x: number; y: number; fromKeyboard: boolean; anchor: HTMLButtonElement } | null>(null);
  const toggleMenu = (fromKeyboard: boolean) => {
    const anchor = buttonRef.current;
    const rect = anchor?.getBoundingClientRect();
    setMenu(menu || !anchor || !rect ? null : { x: rect.left, y: rect.bottom + 2, fromKeyboard, anchor });
  };
  return (
    <>
      <button
        ref={buttonRef}
        disabled={disabled}
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        // Opened from the keyboard (Enter, Space) when the click has no pointer position.
        onClick={(e) => toggleMenu(e.detail === 0)}
        onKeyDown={(e) => {
          if (e.key !== "ArrowDown" || menu) return;
          e.preventDefault();
          toggleMenu(true);
        }}
      >
        {t("sftp.upload")}
        <ChevronIcon />
      </button>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={[
            { label: t("sftp.uploadFiles"), onSelect: () => onUpload(false) },
            { label: t("sftp.uploadFolder"), onSelect: () => onUpload(true) },
          ]}
          anchor={menu.anchor}
          highlightFirst={menu.fromKeyboard}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}
