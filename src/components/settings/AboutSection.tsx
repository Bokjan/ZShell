import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getName, getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";

import { configDirectory } from "../../ipc/api";
import { useSettings } from "../../hooks/settings";
import { useUpdates } from "../../hooks/updates";
import { checkForUpdates } from "../../lib/updates";
import { ErrorText } from "../ErrorMessage";
import { ExternalLinkIcon } from "../icons";
import { FolderField, Section, Setting } from "./common";

const LICENSE_URL = "https://github.com/Bokjan/ZShell/blob/main/LICENSE.md";
const PRIVACY_POLICY_URL = "https://github.com/Bokjan/ZShell/blob/main/PRIVACY.md";

/** The app, its licenses, checking for a newer version, and where its data is. */
export function AboutSection({ onShowLicenses }: { onShowLicenses(): void }) {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  const updates = useUpdates();
  const checkNowRef = useRef<HTMLButtonElement>(null);
  const setUpdates = (patch: Partial<typeof settings.updates>) =>
    update((settings) => ({ ...settings, updates: { ...settings.updates, ...patch } }));
  const [app, setApp] = useState<{ name: string; version: string } | null>(null);
  const [directory, setDirectory] = useState("");
  useEffect(() => {
    Promise.all([getName(), getVersion()])
      .then(([name, version]) => setApp({ name, version }))
      .catch(console.error);
    configDirectory().then(setDirectory).catch(console.error);
  }, []);

  return (
    <Section id="about">
      <Setting>
        {app && (
          <div className="about-app">
            <strong>{app.name}</strong>
            <span>{t("settings.aboutVersion", { version: app.version })}</span>
          </div>
        )}
        <p className="hint">{t("settings.copyright")}</p>
        <div className="about-links">
          <button type="button" className="link" onClick={() => void openUrl(LICENSE_URL).catch(console.error)}>
            {t("settings.license")}
            <ExternalLinkIcon />
            <span className="visually-hidden">{t("common.opensInBrowser")}</span>
          </button>
          <button type="button" className="link" onClick={() => void openUrl(PRIVACY_POLICY_URL).catch(console.error)}>
            {t("settings.privacyPolicy")}
            <ExternalLinkIcon />
            <span className="visually-hidden">{t("common.opensInBrowser")}</span>
          </button>
          <button type="button" className="link" onClick={onShowLicenses}>
            {t("settings.thirdPartyLicenses")}
          </button>
        </div>
      </Setting>
      {updates.supported && (
        <Setting>
          <div className="about-update">
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
            <div className="row about-update-buttons">
              {updates.newer && (
                <button type="button" onClick={() => void openUrl(updates.newer!.url).catch(console.error)}>
                  {t("updates.download")}
                  <ExternalLinkIcon />
                  <span className="visually-hidden">{t("common.opensInBrowser")}</span>
                </button>
              )}
              {updates.available && (
                <button
                  type="button"
                  onClick={() => {
                    setUpdates({ skipped: updates.available!.version });
                    // The button goes.
                    checkNowRef.current?.focus();
                  }}
                >
                  {t("updates.skip")}
                </button>
              )}
              <button ref={checkNowRef} type="button" aria-disabled={updates.checking} onClick={checkForUpdates}>
                {t("updates.checkNow")}
              </button>
            </div>
          </div>
          <label className="checkbox">
            <input type="checkbox" checked={settings.updates.check} onChange={(e) => setUpdates({ check: e.target.checked })} />
            {t("updates.automatic")}
          </label>
          <p className="hint">{t("updates.automaticHint")}</p>
        </Setting>
      )}
      <Setting>
        <FolderField label={t("settings.dataFolder")} path={directory} />
        <p className="hint">{t("settings.dataFolderHint")}</p>
      </Setting>
    </Section>
  );
}
