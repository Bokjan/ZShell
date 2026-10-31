import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";

import { useDialog } from "../hooks/dialogs";
import { useSettings } from "../hooks/settings";
import { useUpdates } from "../hooks/updates";
import { checkForUpdates } from "../lib/updates";
import appIcon from "../../src-tauri/icons/source/icon.svg";
import { ErrorText } from "./ErrorMessage";
import { HelpTip } from "./HelpTip";
import { CloseIcon, ExternalLinkIcon } from "./icons";
import { IconButton } from "./IconButton";
import { LicensesDialog } from "./LicensesDialog";
import { Modal } from "./Modal";

const LICENSE_URL = "https://github.com/Bokjan/ZShell/blob/main/LICENSE.md";
const PRIVACY_POLICY_URL = "https://github.com/Bokjan/ZShell/blob/main/PRIVACY.md";

/**
 * About ZShell, the same on every platform (macOS's own About panel follows the system's
 * language, not the app's): the app, its licenses, and checking for a newer version, which
 * the Microsoft Store package doesn't do. Opened from the app menu (the menu bar on macOS),
 * by Check for Updates, and by the button the session list shows for a newer version.
 */
export function AboutDialog({ onClose }: { onClose(): void }) {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const updates = useUpdates();
  const setUpdates = (patch: Partial<typeof settings.updates>) =>
    update((settings) => ({ ...settings, updates: { ...settings.updates, ...patch } }));
  const checkNowRef = useRef<HTMLButtonElement>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [licensesOpen, setLicensesOpen] = useState(false);
  const dialog = useDialog(onClose, { dismissable: true });

  useEffect(() => {
    getVersion().then(setVersion).catch(console.error);
  }, []);

  const externalLink = (label: string, url: string) => (
    <button type="button" className="link" onClick={() => void openUrl(url).catch(console.error)}>
      {label}
      <ExternalLinkIcon />
      <span className="visually-hidden">{t("common.opensInBrowser")}</span>
    </button>
  );

  return (
    <>
      <Modal dialog={dialog} label={t("appMenu.items.about")}>
        <div className="dialog about-dialog">
          <IconButton className="icon-button about-close" label={t("common.close")} onClick={onClose}>
            <CloseIcon />
          </IconButton>
          <div className="about-app">
            <img src={appIcon} alt="" />
            <h2>{t("app.name")}</h2>
            {version && <p>{t("about.version", { version })}</p>}
            <p className="hint">{t("about.copyright")}</p>
            <div className="about-links">
              {externalLink(t("about.license"), LICENSE_URL)}
              {externalLink(t("about.privacyPolicy"), PRIVACY_POLICY_URL)}
              <button type="button" className="link" onClick={() => setLicensesOpen(true)}>
                {t("about.thirdPartyLicenses")}
              </button>
            </div>
          </div>
          {updates.supported && (
            <div className="about-update">
              {/* A skipped version too, which can still be downloaded here: else a check that finds
                  it again would say nothing. */}
              <p role="status">
                {updates.checking
                  ? t("updates.checking")
                  : updates.newer
                    ? t("updates.available", { version: updates.newer.version })
                    : updates.result?.kind === "upToDate"
                      ? t("updates.upToDate")
                      : ""}
              </p>
              {!updates.checking && updates.result?.kind === "failed" && <ErrorText>{updates.result.message}</ErrorText>}
              <div className="about-links">
                {updates.newer && externalLink(t("updates.download"), updates.newer.url)}
                {updates.available && (
                  <button
                    type="button"
                    className="link"
                    onClick={() => {
                      setUpdates({ skipped: updates.available!.version });
                      // The button goes.
                      checkNowRef.current?.focus();
                    }}
                  >
                    {t("updates.skip")}
                  </button>
                )}
                {/* Always there, so that the focus stays on it whatever the check finds. */}
                <button ref={checkNowRef} type="button" className="link" aria-disabled={updates.checking} onClick={checkForUpdates}>
                  {t("updates.checkNow")}
                </button>
              </div>
              <label className="checkbox">
                <input type="checkbox" checked={settings.updates.check} onChange={(e) => setUpdates({ check: e.target.checked })} />
                {t("updates.automatic")}
                <HelpTip text={t("updates.automaticHint")} />
              </label>
            </div>
          )}
        </div>
      </Modal>
      {licensesOpen && <LicensesDialog onClose={() => setLicensesOpen(false)} />}
    </>
  );
}
