import { useTranslation } from "react-i18next";

import { DEFAULT_TERM_TYPE, ENCODINGS, type CommandGroup } from "../../lib/api";
import type { ProfileForm } from "../../lib/profileForm";
import { groupName } from "../../lib/quickCommands";

/** Suggestions for the terminal type; any value can be typed. */
const TERM_TYPES = [DEFAULT_TERM_TYPE, "xterm", "vt100", "vt220", "linux"];

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
        <label className="grow">
          {t("profile.encoding")}
          <select value={form.encoding} onChange={(e) => set({ encoding: e.target.value })}>
            {ENCODINGS.map((label) => (
              <option key={label} value={label}>
                {t(`profile.encodings.${label}`)}
              </option>
            ))}
          </select>
        </label>
        {protocol !== "serial" && (
          <label className="grow">
            {t("profile.termType")}
            <input
              value={form.termType}
              onChange={(e) => set({ termType: e.target.value })}
              list="profile-term-types"
              placeholder={DEFAULT_TERM_TYPE}
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
            />
            <datalist id="profile-term-types">
              {TERM_TYPES.map((type) => (
                <option key={type} value={type} />
              ))}
            </datalist>
          </label>
        )}
      </div>
      <p className="hint">{t("profile.encodingHint")}</p>
      <label>
        {t("profile.loginCommands")}
        <textarea
          className="command-text"
          rows={3}
          value={form.loginCommands}
          onChange={(e) => set({ loginCommands: e.target.value })}
          placeholder={t("profile.loginCommandsPlaceholder")}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </label>
      <p className="hint">{t("profile.loginCommandsHint")}</p>
      {protocol === "ssh" && (
        <>
          <label>
            {t("profile.env")}
            <textarea
              name="env"
              className="command-text"
              rows={2}
              value={form.env}
              onChange={(e) => set({ env: e.target.value })}
              placeholder={t("profile.envPlaceholder")}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
            />
          </label>
          <p className="hint">{t("profile.envHint")}</p>
        </>
      )}
      <label className="checkbox">
        <input type="checkbox" checked={form.autoLog} onChange={(e) => set({ autoLog: e.target.checked })} />
        {t("profile.autoLog")}
      </label>
      <p className="hint">{t("profile.autoLogHint")}</p>
      <label>
        {t("profile.commandGroup")}
        <select value={shownGroup} disabled={!commandGroups} onChange={(e) => set({ commandGroup: e.target.value })}>
          {(commandGroups ?? []).map((group) => (
            <option key={group.id} value={group.id}>
              {groupName(group, t)}
            </option>
          ))}
        </select>
      </label>
      <p className="hint">{t("profile.commandGroupHint")}</p>
    </>
  );
}
