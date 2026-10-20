import { useTranslation } from "react-i18next";

import { systemLanguage } from "../../i18n/index";
import { LANGUAGES } from "../../ipc/bindings";
import { TYPING_SAVE_DELAY, useSettings, type Appearance, type TextSize } from "../../hooks/settings";
import { languageName } from "../../lib/format";
import { RadioGroup } from "../RadioGroup";
import { Select } from "../Select";
import { LiveTextField, Section, Setting } from "./common";

const APPEARANCES: Appearance[] = ["system", "dark", "light"];
const TEXT_SIZES: TextSize[] = ["normal", "large", "larger"];

/** The interface: light or dark, the size of its text and its font, the status bar and the language. */
export function AppearanceSection() {
  const { t } = useTranslation();
  const { settings, update } = useSettings();
  return (
    <Section id="appearance">
      <RadioGroup
        className="segmented"
        label={t("settings.appearance")}
        value={settings.appearance}
        options={APPEARANCES.map((appearance) => ({ value: appearance, content: t(`settings.appearances.${appearance}`) }))}
        onChange={(appearance) => update((settings) => ({ ...settings, appearance }))}
      />
      <Setting>
        <div className="field">
          <span>{t("settings.textSize")}</span>
          <RadioGroup
            className="segmented"
            label={t("settings.textSize")}
            value={settings.textSize}
            options={TEXT_SIZES.map((textSize) => ({ value: textSize, content: t(`settings.textSizes.${textSize}`) }))}
            onChange={(textSize) => update((settings) => ({ ...settings, textSize }))}
          />
        </div>
        <p className="hint">{t("settings.textSizeHint")}</p>
      </Setting>
      <Setting>
        <div className="row">
          <label className="grow">
            {t("settings.appFont")}
            <LiveTextField
              value={settings.appFont}
              onChange={(appFont) => update((settings) => ({ ...settings, appFont }), TYPING_SAVE_DELAY)}
              placeholder={t("settings.appFontPlaceholder")}
              autoCapitalize="off"
              autoCorrect="off"
            />
          </label>
        </div>
        <p className="hint">{t("settings.appFontHint")}</p>
      </Setting>
      <Setting>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={settings.statusBar.show}
            onChange={(e) => update((settings) => ({ ...settings, statusBar: { ...settings.statusBar, show: e.target.checked } }))}
          />
          {t("settings.showStatusBar")}
        </label>
        <p className="hint">{t("settings.showStatusBarHint")}</p>
      </Setting>
      <Setting>
        <label className="field">
          {t("settings.language")}
          <Select
            value={settings.language}
            onChange={(language) => update((settings) => ({ ...settings, language }))}
            options={[
              { value: "system", label: t("settings.languageSystem", { language: languageName(systemLanguage()) }) },
              ...LANGUAGES.map((language) => ({ value: language, label: languageName(language), lang: language })),
            ]}
          />
        </label>
        <p className="hint">{t("settings.languageHint")}</p>
      </Setting>
    </Section>
  );
}
