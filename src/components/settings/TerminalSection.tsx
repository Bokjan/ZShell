import { useTranslation } from "react-i18next";

import { FONT_SIZE_MAX, FONT_SIZE_MIN, SCROLLBACK_MAX } from "../../ipc/api";
import {
  TYPING_SAVE_DELAY,
  useSettings,
  type CursorStyle,
  type Ligatures,
  type ScreenReaderMode,
  type TerminalSettings,
} from "../../hooks/settings";
import { useLocale } from "../../hooks/locale";
import { TERMINAL_SCHEMES, defaultFontStack, resolveScheme, systemFontFamilies } from "../../lib/terminalSchemes";
import { HelpTip } from "../HelpTip";
import { RadioGroup } from "../RadioGroup";
import { SchemePreview, schemeLabel } from "../SchemePreview";
import { Select } from "../Select";
import { LiveTextField, NumberField, Section, Setting } from "./common";

const CURSOR_STYLES: CursorStyle[] = ["block", "bar", "underline"];
const SCREEN_READER_MODES: ScreenReaderMode[] = ["auto", "on", "off"];
const LIGATURE_MODES: Ligatures[] = ["off", "rules", "full"];

/**
 * The terminals: color scheme, font, ligatures, cursor, scrollback, screen readers, the bell,
 * programs' notifications, and where the times of our own messages are set
 * (`onShowTimeFormat`, with the logs').
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
      <div className="field">
        <span>{t("settings.colorScheme")}</span>
        <RadioGroup
          className="scheme-grid"
          optionClassName="scheme-card"
          label={t("settings.colorScheme")}
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
      </div>

      <Setting>
        <div className="row">
          <label className="grow">
            {t("settings.fontFamily")}
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
          </label>
          <label className="port">
            {t("settings.fontSize")}
            <NumberField value={terminal.fontSize} min={FONT_SIZE_MIN} max={FONT_SIZE_MAX} onChange={(fontSize) => set({ fontSize })} />
          </label>
        </div>
        <p className="hint">{t("settings.fontHint")}</p>
      </Setting>
      <Setting>
        <div className="field">
          <span>{t("settings.ligatures")}</span>
          <RadioGroup
            className="segmented"
            label={t("settings.ligatures")}
            value={terminal.ligatures}
            options={LIGATURE_MODES.map((mode) => ({ value: mode, content: t(`settings.ligatureModes.${mode}`) }))}
            onChange={(ligatures) => set({ ligatures })}
          />
        </div>
        <p className="hint">{t("settings.ligaturesHint")}</p>
      </Setting>

      <Setting>
        <div className="row">
          <label className="grow">
            {t("settings.cursorStyle")}
            <Select
              value={terminal.cursorStyle}
              onChange={(cursorStyle) => set({ cursorStyle })}
              options={CURSOR_STYLES.map((style) => ({ value: style, label: t(`settings.cursorStyles.${style}`) }))}
            />
          </label>
          <label className="grow">
            {t("settings.scrollback")}
            <NumberField
              value={terminal.scrollback}
              min={0}
              max={SCROLLBACK_MAX}
              step={1000}
              onChange={(scrollback) => set({ scrollback })}
            />
          </label>
        </div>
        <p className="hint">{t("settings.scrollbackHint")}</p>
      </Setting>
      <label className="checkbox">
        <input type="checkbox" checked={terminal.cursorBlink} onChange={(e) => set({ cursorBlink: e.target.checked })} />
        {t("settings.cursorBlink")}
      </label>
      <Setting>
        <div className="field">
          <span>{t("settings.screenReader")}</span>
          <RadioGroup
            className="segmented"
            label={t("settings.screenReader")}
            value={terminal.screenReaderMode}
            options={SCREEN_READER_MODES.map((mode) => ({ value: mode, content: t(`settings.screenReaderModes.${mode}`) }))}
            onChange={(screenReaderMode) => set({ screenReaderMode })}
          />
        </div>
        <p className="hint">{t("settings.screenReaderHint")}</p>
      </Setting>
      <Setting>
        <fieldset>
          <legend>{t("settings.bell")}</legend>
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
        </fieldset>
      </Setting>
      <Setting>
        <label className="checkbox">
          <input type="checkbox" checked={terminal.notifications} onChange={(e) => set({ notifications: e.target.checked })} />
          {t("settings.notifications")}
          <HelpTip text={t("settings.notificationsTip")} />
        </label>
      </Setting>
      <Setting>
        <div className="field">
          <span>{t("settings.messageTimes")}</span>
        </div>
        <p className="hint">{t("settings.messageTimesHint")}</p>
        <button type="button" className="link" onClick={onShowTimeFormat}>
          {t("settings.messageTimesFormat")}
        </button>
      </Setting>
    </Section>
  );
}
