import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { isMac } from "../lib/platform";
import { DRAG_REGION } from "../hooks/titleBar";
import { LockIcon } from "./icons";
import { WindowControls } from "./WindowControls";

/**
 * What covers the window while ZShell is locked, by the vault (`UnlockScreen`) or the app lock
 * (`AppLockGate`): a title bar to move the window by, with the window buttons on Windows and Linux, and
 * the panel in the middle.
 */
export function LockScreen({ children }: { children: ReactNode }) {
  return (
    <div className="unlock-screen">
      <div className="unlock-titlebar" {...DRAG_REGION}>
        {!isMac && <WindowControls />}
      </div>
      <main className="unlock-body">{children}</main>
    </div>
  );
}

/** The panel's icon and title. */
export function LockHeading({ id }: { id?: string }) {
  const { t } = useTranslation();
  return (
    <>
      <span className="unlock-icon">
        <LockIcon size={36} />
      </span>
      <h1 id={id}>{t("vault.title")}</h1>
    </>
  );
}
