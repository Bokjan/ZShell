import { useTranslation } from "react-i18next";

import { appearanceOf, type ProfileForm } from "../../lib/profileForm";
import { useSettings } from "../../hooks/settings";
import { FONT_SIZE_MAX, FONT_SIZE_MIN } from "../../ipc/api";
import { sessionScheme, TERMINAL_SCHEMES } from "../../lib/terminalSchemes";
import { ColorField } from "../ColorField";
import { SchemePreview, schemeLabel } from "../SchemePreview";
import { Select } from "../Select";
import { SpinInput } from "../SpinInput";
import { Field } from "../Field";

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
        <Field className="grow" title={t("profile.colorScheme")}>
          <Select
            value={form.colorScheme}
            onChange={(colorScheme) => set({ colorScheme })}
            options={[
              { value: "", label: t("profile.sameAsSettings") },
              { value: "auto", label: t("settings.schemeAuto") },
              ...TERMINAL_SCHEMES.map((scheme) => ({ value: scheme.id, label: schemeLabel(scheme, t) })),
            ]}
          />
        </Field>
        <SchemePreview scheme={preview} />
      </div>
      <div className="row profile-background">
        <label className="checkbox grow">
          <input type="checkbox" checked={form.customBackground} onChange={(e) => set({ customBackground: e.target.checked })} />
          {t("profile.customBackground")}
        </label>
        <ColorField
          value={form.background}
          disabled={!form.customBackground}
          onChange={(background) => set({ background })}
          label={t("profile.customBackground")}
        />
      </div>
      <p className="hint">{t("profile.customBackgroundHint")}</p>
      <div className="row">
        <Field className="grow" title={t("settings.fontFamily")}>
          <input
            value={form.fontFamily}
            onChange={(e) => set({ fontFamily: e.target.value })}
            placeholder={settings.terminal.fontFamily || t("profile.sameAsSettings")}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </Field>
        <Field className="port" title={t("settings.fontSize")}>
          <SpinInput
            name="fontSize"
            value={form.fontSize}
            onChange={(fontSize) => set({ fontSize })}
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            start={settings.terminal.fontSize}
            placeholder={String(settings.terminal.fontSize)}
          />
        </Field>
      </div>
    </>
  );
}
