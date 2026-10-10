import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { IconButton } from "./IconButton";
import { CloseIcon } from "./icons";

/**
 * A problem shown in a dialog, such as a field that isn't valid or a save that failed. It is an
 * alert, so screen readers say it when it appears or changes, wherever the focus is (often on
 * the button that was pressed).
 */
export function ErrorText({ children, className = "error" }: { children: ReactNode; className?: string }) {
  return (
    <p className={className} role="alert">
      {children}
    </p>
  );
}

/**
 * A problem shown at the top of a panel until it is dismissed: with its button, which the
 * keyboard reaches, or by clicking anywhere on it. Said by screen readers when it appears.
 */
export function ErrorBanner({ children, onDismiss }: { children: ReactNode; onDismiss(): void }) {
  const { t } = useTranslation();
  return (
    <div className="panel-error dismissable" role="alert" onClick={onDismiss}>
      <span>{children}</span>
      <IconButton
        className="panel-error-dismiss"
        label={t("common.dismiss")}
        onClick={(e) => {
          e.stopPropagation();
          onDismiss();
        }}
      >
        <CloseIcon size={12} />
      </IconButton>
    </div>
  );
}
