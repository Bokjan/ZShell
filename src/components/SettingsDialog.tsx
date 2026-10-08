import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { isMac } from "../lib/platform";
import {
  DEFAULT_SETTINGS,
  useSettings,
  type Appearance,
  type CursorStyle,
  type RightClick,
  type SidebarSettings,
  type TabSettings,
  type TerminalSettings,
  type ZmodemSettings,
} from "../lib/settings";
import { DEFAULT_FONT_STACK, TERMINAL_SCHEMES, resolveScheme, type TerminalScheme } from "../lib/terminalSchemes";

interface Props {
  onClose(): void;
}

const APPEARANCES: Appearance[] = ["system", "dark", "light"];
const CURSOR_STYLES: CursorStyle[] = ["block", "bar", "underline"];
const RIGHT_CLICKS: RightClick[] = ["menu", "paste"];

/** A number field that only reports values that are integers within range. */
function NumberField({ value, min, max, onChange }: { value: number; min: number; max: number; onChange(n: number): void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const valid = (n: number) => Number.isInteger(n) && n >= min && n <= max;
  return (
    <input
      value={text}
      inputMode="numeric"
      aria-invalid={!valid(Number(text))}
      onChange={(e) => {
        setText(e.target.value);
        const n = Number(e.target.value);
        if (e.target.value.trim() !== "" && valid(n)) onChange(n);
      }}
      onBlur={() => setText(String(value))}
    />
  );
}

/** Miniature terminal in the scheme's colors. */
function SchemePreview({ scheme }: { scheme: TerminalScheme }) {
  const t = scheme.theme;
  return (
    <span className="scheme-preview" style={{ background: t.background, color: t.foreground }}>
      <span className="scheme-prompt">
        <span style={{ color: t.green }}>~</span> <span style={{ color: t.blue }}>$</span> ls
      </span>
      <span className="scheme-swatches">
        {[t.red, t.green, t.yellow, t.blue, t.magenta, t.cyan].map((color, i) => (
          <span key={i} style={{ background: color }} />
        ))}
      </span>
    </span>
  );
}

export function SettingsDialog({ onClose }: Props) {
  const { t } = useTranslation();
  const { settings, theme, update } = useSettings();
  const terminal = settings.terminal;
  const setTerminal = (patch: Partial<TerminalSettings>) => update({ ...settings, terminal: { ...terminal, ...patch } });
  const setTabs = (patch: Partial<TabSettings>) => update({ ...settings, tabs: { ...settings.tabs, ...patch } });
  const setSidebar = (patch: Partial<SidebarSettings>) => update({ ...settings, sidebar: { ...settings.sidebar, ...patch } });
  const setZmodem = (patch: Partial<ZmodemSettings>) => update({ ...settings, zmodem: { ...settings.zmodem, ...patch } });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const schemeName = (scheme: TerminalScheme) =>
    scheme.name ?? (scheme.dark ? t("settings.schemeDefaultDark") : t("settings.schemeDefaultLight"));

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog settings-dialog" role="dialog" aria-label={t("settings.title")}>
        <h2>{t("settings.title")}</h2>

        <section>
          <h3>{t("settings.appearance")}</h3>
          <div className="segmented" role="radiogroup">
            {APPEARANCES.map((appearance) => (
              <button
                key={appearance}
                role="radio"
                aria-checked={settings.appearance === appearance}
                className={settings.appearance === appearance ? "on" : undefined}
                onClick={() => update({ ...settings, appearance })}
              >
                {t(`settings.appearances.${appearance}`)}
              </button>
            ))}
          </div>
        </section>

        <section>
          <h3>{t("settings.terminal")}</h3>
          <div className="field">
            <span>{t("settings.colorScheme")}</span>
            <div className="scheme-grid" role="radiogroup">
              <button
                role="radio"
                aria-checked={terminal.colorScheme === "auto"}
                className={`scheme-card${terminal.colorScheme === "auto" ? " on" : ""}`}
                onClick={() => setTerminal({ colorScheme: "auto" })}
              >
                <SchemePreview scheme={resolveScheme("auto", theme)} />
                <span className="scheme-name">{t("settings.schemeAuto")}</span>
              </button>
              {TERMINAL_SCHEMES.map((scheme) => (
                <button
                  key={scheme.id}
                  role="radio"
                  aria-checked={terminal.colorScheme === scheme.id}
                  className={`scheme-card${terminal.colorScheme === scheme.id ? " on" : ""}`}
                  onClick={() => setTerminal({ colorScheme: scheme.id })}
                >
                  <SchemePreview scheme={scheme} />
                  <span className="scheme-name">{schemeName(scheme)}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="row">
            <label className="grow">
              {t("settings.fontFamily")}
              <input
                value={terminal.fontFamily}
                onChange={(e) => setTerminal({ fontFamily: e.target.value })}
                placeholder={t("settings.fontFamilyPlaceholder")}
                title={DEFAULT_FONT_STACK}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
              />
            </label>
            <label className="port">
              {t("settings.fontSize")}
              <NumberField value={terminal.fontSize} min={6} max={48} onChange={(fontSize) => setTerminal({ fontSize })} />
            </label>
          </div>
          <p className="hint">{t("settings.fontHint")}</p>

          <div className="row">
            <label className="grow">
              {t("settings.cursorStyle")}
              <select
                value={terminal.cursorStyle}
                onChange={(e) => setTerminal({ cursorStyle: e.target.value as CursorStyle })}
              >
                {CURSOR_STYLES.map((style) => (
                  <option key={style} value={style}>
                    {t(`settings.cursorStyles.${style}`)}
                  </option>
                ))}
              </select>
            </label>
            <label className="grow">
              {t("settings.scrollback")}
              <NumberField
                value={terminal.scrollback}
                min={0}
                max={100000}
                onChange={(scrollback) => setTerminal({ scrollback })}
              />
            </label>
          </div>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={terminal.cursorBlink}
              onChange={(e) => setTerminal({ cursorBlink: e.target.checked })}
            />
            {t("settings.cursorBlink")}
          </label>
        </section>

        <section>
          <h3>{t("settings.mouseAndClipboard")}</h3>
          <div className="field">
            <span>{t("settings.rightClick")}</span>
            <div className="segmented" role="radiogroup">
              {RIGHT_CLICKS.map((rightClick) => (
                <button
                  key={rightClick}
                  role="radio"
                  aria-checked={terminal.rightClick === rightClick}
                  className={terminal.rightClick === rightClick ? "on" : undefined}
                  onClick={() => setTerminal({ rightClick })}
                >
                  {t(`settings.rightClicks.${rightClick}`)}
                </button>
              ))}
            </div>
          </div>
          <p className="hint">{t("settings.rightClickHint")}</p>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={terminal.copyOnSelect}
              onChange={(e) => setTerminal({ copyOnSelect: e.target.checked })}
            />
            {t("settings.copyOnSelect")}
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={terminal.confirmMultilinePaste}
              onChange={(e) => setTerminal({ confirmMultilinePaste: e.target.checked })}
            />
            {t("settings.confirmMultilinePaste")}
          </label>
          <p className="hint">{t("settings.confirmMultilinePasteHint")}</p>
          {isMac && (
            <>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={terminal.optionAsMeta}
                  onChange={(e) => setTerminal({ optionAsMeta: e.target.checked })}
                />
                {t("settings.optionAsMeta")}
              </label>
              <p className="hint">{t("settings.optionAsMetaHint")}</p>
            </>
          )}
        </section>

        <section>
          <h3>{t("settings.tabs")}</h3>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.tabs.followRemoteTitle}
              onChange={(e) => setTabs({ followRemoteTitle: e.target.checked })}
            />
            {t("settings.followRemoteTitle")}
          </label>
          <p className="hint">{t("settings.followRemoteTitleHint")}</p>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.tabs.confirmClose}
              onChange={(e) => setTabs({ confirmClose: e.target.checked })}
            />
            {t("settings.confirmClose")}
          </label>
        </section>

        <section>
          <h3>{t("settings.sidebar")}</h3>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.sidebar.showRecent}
              onChange={(e) => setSidebar({ showRecent: e.target.checked })}
            />
            {t("settings.showRecent")}
          </label>
        </section>

        <section>
          <h3>{t("settings.zmodem")}</h3>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.zmodem.askDownloadLocation}
              onChange={(e) => setZmodem({ askDownloadLocation: e.target.checked })}
            />
            {t("settings.zmodemAskLocation")}
          </label>
          <p className="hint">{t("settings.zmodemAskLocationHint")}</p>
        </section>

        <footer>
          <button type="button" onClick={() => update(DEFAULT_SETTINGS)}>
            {t("settings.reset")}
          </button>
          <span className="grow" />
          <button type="button" className="primary" onClick={onClose} autoFocus>
            {t("settings.done")}
          </button>
        </footer>
      </div>
    </div>
  );
}
