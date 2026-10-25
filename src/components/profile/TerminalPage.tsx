import { useTranslation } from "react-i18next";

import { DEFAULT_TERM_TYPE, ENCODINGS, type CommandGroup } from "../../ipc/api";
import type { ProfileForm } from "../../lib/profileForm";
import { groupName } from "../../lib/quickCommands";
import { AutoTextarea } from "../AutoTextarea";
import { ComboBox } from "../ComboBox";
import { Select } from "../Select";
import { Field } from "../Field";

/** Suggestions for the terminal type; any value can be typed. */
const TERM_TYPES = [DEFAULT_TERM_TYPE, "xterm", "vt100", "vt220", "linux"].map((value) => ({ value }));

interface Props {
  form: ProfileForm;
  set(patch: Partial<ProfileForm>): void;
  /** The quick command groups; null until they have loaded. */
  commandGroups: CommandGroup[] | null;
  /** The group shown: the session's, or the default for one deleted since. */
  shownGroup: string;
}

/** The session dialog's page of what its shell gets: encoding, terminal type, commands, logging. */
export function TerminalPage({ form, set, commandGroups, shownGroup }: Props) {
  const { t } = useTranslation();
  const { protocol } = form;
  return (
    <>
      <div className="row">
        <Field className="grow" title={t("profile.encoding")}>
          <Select
            value={form.encoding}
            onChange={(encoding) => set({ encoding })}
            options={ENCODINGS.map((value) => ({ value, label: t(`profile.encodings.${value}`) }))}
          />
        </Field>
        {protocol !== "serial" && (
          <Field className="grow" title={t("profile.termType")}>
            <ComboBox
              value={form.termType}
              onChange={(termType) => set({ termType })}
              suggestions={TERM_TYPES}
              placeholder={DEFAULT_TERM_TYPE}
              autoCapitalize="off"
              autoCorrect="off"
            />
          </Field>
        )}
      </div>
      <p className="hint">{t("profile.encodingHint")}</p>
      <Field title={t("profile.loginCommands")} hint={t("profile.loginCommandsHint")}>
        <AutoTextarea
          className="command-text"
          rows={3}
          value={form.loginCommands}
          onChange={(e) => set({ loginCommands: e.target.value })}
          placeholder={t("profile.loginCommandsPlaceholder")}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </Field>
      {protocol === "ssh" && (
        <>
          <Field title={t("profile.env")} hint={t("profile.envHint")}>
            <AutoTextarea
              name="env"
              className="command-text"
              rows={2}
              value={form.env}
              onChange={(e) => set({ env: e.target.value })}
              placeholder={t("profile.envPlaceholder")}
              autoCapitalize="off"
              autoCorrect="off"
            />
          </Field>
        </>
      )}
      <label className="checkbox">
        <input type="checkbox" checked={form.autoLog} onChange={(e) => set({ autoLog: e.target.checked })} />
        {t("profile.autoLog")}
      </label>
      <p className="hint">{t("profile.autoLogHint")}</p>
      <Field title={t("profile.commandGroup")} hint={t("profile.commandGroupHint")}>
        <Select
          value={shownGroup}
          disabled={!commandGroups}
          onChange={(commandGroup) => set({ commandGroup })}
          options={(commandGroups ?? []).map((group) => ({ value: group.id, label: groupName(group, t) }))}
        />
      </Field>
    </>
  );
}
