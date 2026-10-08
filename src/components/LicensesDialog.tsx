import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

/** Written by `pnpm licenses:generate`; local builds may not have it. */
const NOTICES_URL = "/third-party-licenses.txt";

/** The third-party license notices that ship with the app. */
export function LicensesDialog({ onClose }: { onClose(): void }) {
  const { t } = useTranslation();
  // null while loading, "" when the build has no notices.
  const [text, setText] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    fetch(NOTICES_URL)
      // Both the dev server and Tauri answer a missing file with index.html.
      .then((res) => (res.ok && !res.headers.get("content-type")?.includes("text/html") ? res.text() : ""))
      .catch(() => "")
      .then((notices) => current && setText(notices));
    return () => {
      current = false;
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog licenses-dialog" role="dialog" aria-label={t("settings.thirdPartyLicenses")}>
        <h2>{t("settings.thirdPartyLicenses")}</h2>
        {text === "" ? (
          <p className="dialog-message">{t("settings.licensesMissing")}</p>
        ) : (
          <pre className="licenses-text">{text}</pre>
        )}
        <footer>
          <span className="grow" />
          <button type="button" className="primary" onClick={onClose} autoFocus>
            {t("common.close")}
          </button>
        </footer>
      </div>
    </div>
  );
}
