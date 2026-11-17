import { useTranslation } from "react-i18next";

import { FONT_SIZE_MAX, FONT_SIZE_MIN, SCROLLBACK_MAX, type LocalLaunch } from "../../ipc/api";
import {
  TYPING_SAVE_DELAY,
  useSettings,
  type CursorStyle,
  type Ligatures,
  type ScreenReaderMode,
  type TerminalSettings,
} from "../../hooks/settings";
import { useLocale } from "../../hooks/locale";
import { loadLocalShells, useLocalAllowed, useLocalShells } from "../../hooks/localShells";
import { useDirectory } from "../../hooks/stores";
import { foundName, localOptions, sameShell, shellTitle } from "../../lib/localShells";
import { TERMINAL_SCHEMES, defaultFontStack, resolveScheme, systemFontFamilies } from "../../lib/terminalSchemes";
import { HelpTip } from "../HelpTip";
import { IconButton } from "../IconButton";
import { RefreshIcon } from "../icons";
import { RadioGroup } from "../RadioGroup";
import { SchemePreview, schemeLabel } from "../SchemePreview";
import { Select } from "../Select";
import { LiveTextField, NumberField, Section, Setting } from "./common";
import { Field } from "../Field";

const CURSOR_STYLES: CursorStyle[] = ["block", "bar", "underline"];
const SCREEN_READER_MODES: ScreenReaderMode[] = ["auto", "on", "off"];
const LIGATURE_MODES: Ligatures[] = ["off", "rules", "full"];

/**
 * The terminals: color scheme, font, ligatures, cursor, scrollback, screen readers, the bell,
 * programs' notifications, where the times of our own messages are set (`onShowTimeFormat`,
 * with the logs'), and the local terminal "+" opens.
 */
export function TerminalSection({ onShowTimeFormat }: { onShowTimeFormat(): void }) {
  const { t } = useTranslation();
  const { cjk } = useLocale();
  const { settings, theme, update } = useSettings();
  const terminal = settings.terminal;
  const set = (patch: Partial<TerminalSettings>, delay?: number) =>
    update((settings) => ({ ...settings, terminal: { ...settings.terminal, ...patch } }), delay);
  return (
    <Section id="terminal">
      <Field group title={t("settings.colorScheme")}>
        <RadioGroup
          className="scheme-grid"
          optionClassName="scheme-card"
          value={terminal.colorScheme}
          options={[
            {
              value: "auto",
              content: (
                <>
                  <SchemePreview scheme={resolveScheme("auto", theme)} />
                  <span className="scheme-name">{t("settings.schemeAuto")}</span>
                </>
              ),
            },
            ...TERMINAL_SCHEMES.map((scheme) => ({
              value: scheme.id,
              content: (
                <>
                  <SchemePreview scheme={scheme} />
                  <span className="scheme-name">{schemeLabel(scheme, t)}</span>
                </>
              ),
            })),
          ]}
          onChange={(colorScheme) => set({ colorScheme })}
        />
      </Field>

      <Setting>
        <div className="row">
          <Field className="grow" title={t("settings.fontFamily")}>
            <LiveTextField
              value={terminal.fontFamily}
              onChange={(fontFamily) => set({ fontFamily }, TYPING_SAVE_DELAY)}
              placeholder={
                systemFontFamilies()
                  ? t("settings.fontFamilyPlaceholderSystem", { font: systemFontFamilies().replace(/"/g, "") })
                  : t("settings.fontFamilyPlaceholder")
              }
              title={defaultFontStack(cjk)}
              autoCapitalize="off"
              autoCorrect="off"
            />
          </Field>
          <Field className="port" title={t("settings.fontSize")}>
            <NumberField value={terminal.fontSize} min={FONT_SIZE_MIN} max={FONT_SIZE_MAX} onChange={(fontSize) => set({ fontSize })} />
          </Field>
        </div>
        <p className="hint">{t("settings.fontHint")}</p>
      </Setting>
      <Setting>
        <Field group title={t("settings.ligatures")} hint={t("settings.ligaturesHint")}>
          <RadioGroup
            className="segmented"
            value={terminal.ligatures}
            options={LIGATURE_MODES.map((mode) => ({ value: mode, content: t(`settings.ligatureModes.${mode}`) }))}
            onChange={(ligatures) => set({ ligatures })}
          />
        </Field>
      </Setting>

      <Setting>
        <div className="row">
          <Field className="grow" title={t("settings.cursorStyle")}>
            <Select
              value={terminal.cursorStyle}
              onChange={(cursorStyle) => set({ cursorStyle })}
              options={CURSOR_STYLES.map((style) => ({ value: style, label: t(`settings.cursorStyles.${style}`) }))}
            />
          </Field>
          <Field className="grow" title={t("settings.scrollback")}>
            <NumberField
              value={terminal.scrollback}
              min={0}
              max={SCROLLBACK_MAX}
              step={1000}
              onChange={(scrollback) => set({ scrollback })}
            />
          </Field>
        </div>
        <p className="hint">{t("settings.scrollbackHint")}</p>
      </Setting>
      <label className="checkbox">
        <input type="checkbox" checked={terminal.cursorBlink} onChange={(e) => set({ cursorBlink: e.target.checked })} />
        {t("settings.cursorBlink")}
      </label>
      <Setting>
        <Field group title={t("settings.screenReader")} hint={t("settings.screenReaderHint")}>
          <RadioGroup
            className="segmented"
            value={terminal.screenReaderMode}
            options={SCREEN_READER_MODES.map((mode) => ({ value: mode, content: t(`settings.screenReaderModes.${mode}`) }))}
            onChange={(screenReaderMode) => set({ screenReaderMode })}
          />
        </Field>
      </Setting>
      <Setting>
        <Field group title={t("settings.bell")}>
          <label className="checkbox">
            <input type="checkbox" checked={terminal.bellAlert} onChange={(e) => set({ bellAlert: e.target.checked })} />
            {t("settings.bellAlert")}
          </label>
          <p className="hint">{t("settings.bellAlertHint")}</p>
          <label className="checkbox">
            <input type="checkbox" checked={terminal.bellSound} onChange={(e) => set({ bellSound: e.target.checked })} />
            {t("settings.bellSound")}
          </label>
          <label className="checkbox">
            <input type="checkbox" checked={terminal.bellFlash} onChange={(e) => set({ bellFlash: e.target.checked })} />
            {t("settings.bellFlash")}
          </label>
        </Field>
      </Setting>
      <Setting>
        <label className="checkbox">
          <input type="checkbox" checked={terminal.notifications} onChange={(e) => set({ notifications: e.target.checked })} />
          {t("settings.notifications")}
          <HelpTip text={t("settings.notificationsTip")} />
        </label>
      </Setting>
      <Setting>
        <Field group title={t("settings.messageTimes")}>
          <p className="hint">{t("settings.messageTimesHint")}</p>
          <button type="button" className="link" onClick={onShowTimeFormat}>
            {t("settings.messageTimesFormat")}
          </button>
        </Field>
      </Setting>
      <DefaultLocalShell value={terminal.localShell} onChange={(localShell) => set({ localShell })} />
    </Section>
  );
}

/**
 * The local terminal "+" opens: the default shell, one of the shells found here (found again
 * with the button beside), or a saved local session. Not where the system blocks local terminals.
 */
function DefaultLocalShell({ value, onChange }: { value: LocalLaunch | null; onChange(value: LocalLaunch | null): void }) {
  const { t } = useTranslation();
  const allowed = useLocalAllowed();
  const shells = useLocalShells() ?? [];
  const sessions = useDirectory().profiles.filter((profile) => localOptions(profile));
  if (!allowed) return null;
  // A session gone, or no longer local, opens the default shell, as it shows.
  const shellIndex = value?.kind === "shell" ? shells.findIndex((found) => sameShell(found.shell, value.shell)) : -1;
  const chosen =
    value?.kind === "shell"
      ? shellIndex >= 0
        ? `shell:${shellIndex}`
        : "set"
      : value && sessions.some((p) => p.id === value.profileId)
        ? `session:${value.profileId}`
        : "default";
  const options = [
    { value: "default", label: t("profile.defaultShell") },
    // One set that isn't found here (any more): as it was set.
    ...(chosen === "set" && value?.kind === "shell"
      ? [{ value: "set", label: shellTitle(value.shell, shells, t("profile.defaultShell"), t) }]
      : []),
    ...shells.map((found, i) => ({ value: `shell:${i}`, label: foundName(found, t) })),
    ...sessions.map((profile) => ({ value: `session:${profile.id}`, label: t("settings.localShellSession", { name: profile.name }) })),
  ];
  const choose = (option: string) => {
    if (option === "default") onChange(null);
    else if (option.startsWith("shell:")) onChange({ kind: "shell", shell: shells[Number(option.slice(6))].shell });
    else if (option.startsWith("session:")) onChange({ kind: "session", profileId: option.slice(8) });
  };
  return (
    <Setting>
      <Field title={t("settings.localShell")} hint={t("settings.localShellHint")}>
        <div className="input-with-button">
          <Select value={chosen} onChange={choose} options={options} />
          <IconButton type="button" className="icon-button" label={t("settings.refreshShells")} onClick={() => void loadLocalShells(true)}>
            <RefreshIcon />
          </IconButton>
        </div>
      </Field>
    </Setting>
  );
}
