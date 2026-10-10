import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getName, getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";

import { configDirectory } from "../../lib/api";
import { isMac } from "../../lib/platform";
import { ExternalLinkIcon } from "../icons";
import { FolderField, Section, Setting } from "./common";

const LICENSE_URL = "https://github.com/Bokjan/ZShell/blob/main/LICENSE.md";
const PRIVACY_POLICY_URL = "https://github.com/Bokjan/ZShell/blob/main/PRIVACY.md";

/** The app, its licenses, and where its data is. */
export function AboutSection({ onShowLicenses }: { onShowLicenses(): void }) {
  const { t } = useTranslation();
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
      <Setting>
        <FolderField label={t("settings.dataFolder")} path={directory} />
        <p className="hint">{t(isMac ? "settings.dataFolderHint" : "settings.dataFolderHintWindows")}</p>
      </Setting>
    </Section>
  );
}
