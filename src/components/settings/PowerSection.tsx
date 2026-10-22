import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { appSettings } from "../../ipc/api";
import type { KeepAwakeMode, PowerSettings } from "../../hooks/settings";
import { Select } from "../Select";
import { Section, Setting } from "./common";

const KEEP_AWAKE: KeepAwakeMode[] = ["off", "pluggedIn", "always"];

/** Keeping the system from sleeping while sessions are connected. */
export function PowerSection({ settings, onChange }: { settings: PowerSettings; onChange(patch: Partial<PowerSettings>): void }) {
  const { t } = useTranslation();
  // Windows with Modern Standby ends the hold on battery soon after the sleep time.
  const [limited, setLimited] = useState(false);
  useEffect(() => {
    appSettings.keepAwakeLimited().then(setLimited).catch(console.error);
  }, []);

  return (
    <Section id="power">
      <Setting>
        <label>
          {t("settings.keepAwake")}
          <Select
            value={settings.keepAwake}
            onChange={(keepAwake) => onChange({ keepAwake })}
            options={KEEP_AWAKE.map((mode) => ({ value: mode, label: t(`settings.keepAwakeOptions.${mode}`) }))}
          />
        </label>
        <p className="hint">{t("settings.keepAwakeHint")}</p>
        {limited && settings.keepAwake === "always" && <p className="hint">{t("settings.keepAwakeLimited")}</p>}
      </Setting>
    </Section>
  );
}
