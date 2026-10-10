import { useTranslation } from "react-i18next";

import { appearanceOf, type ProfileForm } from "../../lib/profileForm";
import { FONT_SIZE_MAX, FONT_SIZE_MIN, useSettings } from "../../lib/settings";
import { sessionScheme, TERMINAL_SCHEMES } from "../../lib/terminalSchemes";
import { SchemePreview, schemeLabel } from "../SchemePreview";
import { SpinInput } from "../SpinInput";

interface Props {
  form: ProfileForm;
  set(patch: Partial<ProfileForm>): void;
}

/** The session dialog's page of how its terminal looks, where it differs from the settings. */
export function AppearancePage({ form, set }: Props) {
  const { t } = useTranslation();
  const { settings, theme } = useSettings();
  const preview = sessionScheme(settings.terminal.colorScheme, theme, appearanceOf(form));
  return (
    <>
      <p className="hint profile-page-hint">{t("profile.appearanceHint")}</p>
      <div className="row profile-scheme">
        <label className="grow">
          {t("profile.colorScheme")}
          <select value={form.colorScheme} onChange={(e) => set({ colorScheme: e.target.value })}>
            <option value="">{t("profile.sameAsSettings")}</option>
            <option value="auto">{t("settings.schemeAuto")}</option>
            {TERMINAL_SCHEMES.map((scheme) => (
              <option key={scheme.id} value={scheme.id}>
                {schemeLabel(scheme, t)}
              </option>
            ))}
          </select>
        </label>
        <SchemePreview scheme={preview} />
      </div>
      <div className="row profile-background">
        <label className="checkbox grow">
          <input type="checkbox" checked={form.customBackground} onChange={(e) => set({ customBackground: e.target.checked })} />
          {t("profile.customBackground")}
        </label>
        <input
          type="color"
          value={form.background}
          disabled={!form.customBackground}
          onChange={(e) => set({ background: e.target.value })}
          aria-label={t("profile.customBackground")}
        />
      </div>
      <p className="hint">{t("profile.customBackgroundHint")}</p>
      <div className="row">
        <label className="grow">
          {t("settings.fontFamily")}
          <input
            value={form.fontFamily}
            onChange={(e) => set({ fontFamily: e.target.value })}
            placeholder={settings.terminal.fontFamily || t("profile.sameAsSettings")}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <label className="port">
          {t("settings.fontSize")}
          <SpinInput
            name="fontSize"
            value={form.fontSize}
            onChange={(fontSize) => set({ fontSize })}
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            start={settings.terminal.fontSize}
            placeholder={String(settings.terminal.fontSize)}
          />
        </label>
      </div>
    </>
  );
}
