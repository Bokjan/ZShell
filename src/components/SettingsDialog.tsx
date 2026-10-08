import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getName, getVersion } from "@tauri-apps/api/app";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

import { logs, sftp } from "../lib/api";
import { basename, formatSize } from "../lib/format";
import { isMac } from "../lib/platform";
import {
  DEFAULT_SETTINGS,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  useSettings,
  type Appearance,
  type CursorStyle,
  type FileSettings,
  type LogFormat,
  type LogSettings,
  type RightClick,
  type SidebarSettings,
  type TabSettings,
  type TerminalSettings,
  type ZmodemSettings,
} from "../lib/settings";
import { DEFAULT_FONT_STACK, TERMINAL_SCHEMES, resolveScheme, type TerminalScheme } from "../lib/terminalSchemes";
import { LicensesDialog } from "./LicensesDialog";
import { SchemePreview, schemeLabel } from "./SchemePreview";

interface Props {
  /** Whether the system allows local terminals (not Windows in S mode). */
  localAllowed: boolean;
  onClose(): void;
}

const APPEARANCES: Appearance[] = ["system", "dark", "light"];
const CURSOR_STYLES: CursorStyle[] = ["block", "bar", "underline"];
const RIGHT_CLICKS: RightClick[] = ["menu", "paste"];
const LOG_FORMATS: LogFormat[] = ["text", "raw"];
/** Retention choices, in days; 0 keeps logs. */
const KEEP_DAYS = [0, 7, 30, 90, 365];
const PRIVACY_POLICY_URL = "https://github.com/Bokjan/ZShell/blob/main/PRIVACY.md";

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

export function SettingsDialog({ localAllowed, onClose }: Props) {
  const { t } = useTranslation();
  const { settings, theme, update } = useSettings();
  const terminal = settings.terminal;
  const setTerminal = (patch: Partial<TerminalSettings>) => update({ ...settings, terminal: { ...terminal, ...patch } });
  const setTabs = (patch: Partial<TabSettings>) => update({ ...settings, tabs: { ...settings.tabs, ...patch } });
  const setSidebar = (patch: Partial<SidebarSettings>) => update({ ...settings, sidebar: { ...settings.sidebar, ...patch } });
  const setFiles = (patch: Partial<FileSettings>) => update({ ...settings, files: { ...settings.files, ...patch } });
  const setZmodem = (patch: Partial<ZmodemSettings>) => update({ ...settings, zmodem: { ...settings.zmodem, ...patch } });
  const setLogs = (patch: Partial<LogSettings>) => update({ ...settings, logs: { ...settings.logs, ...patch } });
  const [licensesOpen, setLicensesOpen] = useState(false);

  // While the licenses are open, Escape closes only them.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !licensesOpen && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, licensesOpen]);

  const schemeName = (scheme: TerminalScheme) => schemeLabel(scheme, t);

  return (
    <>
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
                <NumberField
                  value={terminal.fontSize}
                  min={FONT_SIZE_MIN}
                  max={FONT_SIZE_MAX}
                  onChange={(fontSize) => setTerminal({ fontSize })}
                />
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

          <FileSection settings={settings.files} onChange={setFiles} />

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

          <LogSection settings={settings.logs} localAllowed={localAllowed} onChange={setLogs} />

          <AboutSection onShowLicenses={() => setLicensesOpen(true)} />

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
      {licensesOpen && <LicensesDialog onClose={() => setLicensesOpen(false)} />}
    </>
  );
}

/** Remote files: where downloads go, and the editor remote files are edited with. */
function FileSection({ settings, onChange }: { settings: FileSettings; onChange(patch: Partial<FileSettings>): void }) {
  const { t } = useTranslation();
  // The folder in effect (Downloads when none is chosen).
  const [directory, setDirectory] = useState("");
  useEffect(() => {
    sftp.downloadsDirectory().then(setDirectory).catch(console.error);
  }, [settings.downloadDirectory]);

  const chooseDirectory = async () => {
    const picked = await openDialog({ directory: true, defaultPath: directory || undefined }).catch(() => null);
    if (typeof picked === "string") onChange({ downloadDirectory: picked });
  };

  const chooseEditor = async () => {
    const picked = await openDialog({
      filters: [isMac ? { name: t("settings.editorApplications"), extensions: ["app"] } : { name: t("settings.editorPrograms"), extensions: ["exe"] }],
      defaultPath: isMac ? "/Applications" : undefined,
    }).catch(() => null);
    if (typeof picked === "string") onChange({ editor: picked });
  };

  return (
    <section>
      <h3>{t("settings.files")}</h3>
      <div className="field">
        <span>{t("settings.downloadDirectory")}</span>
        <div className="row">
          <input className="grow" value={directory} readOnly title={directory} />
          <button type="button" onClick={() => void chooseDirectory()}>
            {t("settings.logChoose")}
          </button>
          <button type="button" onClick={() => revealItemInDir(directory).catch(console.error)} disabled={!directory}>
            {t("settings.logShow")}
          </button>
        </div>
      </div>
      {settings.downloadDirectory && (
        <button type="button" className="link" onClick={() => onChange({ downloadDirectory: "" })}>
          {t("settings.downloadDirectoryDefault")}
        </button>
      )}
      <p className="hint">{t("settings.downloadDirectoryHint")}</p>
      <div className="field">
        <span>{t("settings.editor")}</span>
        <div className="row">
          <input
            className="grow"
            value={settings.editor ? basename(settings.editor).replace(/\.(app|exe)$/i, "") : t("settings.editorDefault")}
            readOnly
            title={settings.editor}
          />
          <button type="button" onClick={() => void chooseEditor()}>
            {t("settings.logChoose")}
          </button>
        </div>
      </div>
      {settings.editor && (
        <button type="button" className="link" onClick={() => onChange({ editor: "" })}>
          {t("settings.editorUseDefault")}
        </button>
      )}
      <p className="hint">{t("settings.editorHint")}</p>
    </section>
  );
}

/** Session logs: where and how they are written, how long they are kept, deleting them. */
function LogSection({
  settings,
  localAllowed,
  onChange,
}: {
  settings: LogSettings;
  localAllowed: boolean;
  onChange(patch: Partial<LogSettings>): void;
}) {
  const { t } = useTranslation();
  const [directory, setDirectory] = useState("");
  const [summary, setSummary] = useState<{ count: number; bytes: number } | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [fileName, setFileName] = useState(settings.fileName);
  useEffect(() => setFileName(settings.fileName), [settings.fileName]);

  const refresh = useCallback(() => {
    logs.summary().then(setSummary).catch(console.error);
  }, []);
  useEffect(refresh, [refresh]);
  // The folder in effect (the default one when none is chosen).
  useEffect(() => {
    logs.directory().then(setDirectory).catch(console.error);
  }, [settings.directory]);

  const choose = async () => {
    const picked = await openDialog({ directory: true, defaultPath: directory || undefined }).catch(() => null);
    if (typeof picked === "string") onChange({ directory: picked });
  };

  const deleteAll = () => {
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      return;
    }
    setConfirmingDelete(false);
    logs.delete().then(refresh, console.error);
  };

  // The file name is saved when the field is left, so that clearing it to type another
  // doesn't bring the default back in the middle.
  const commitFileName = () => {
    if (fileName.trim() !== settings.fileName) onChange({ fileName: fileName.trim() });
  };

  return (
    <section>
      <h3>{t("settings.logs")}</h3>
      <div className="field">
        <span>{t("settings.logDirectory")}</span>
        <div className="row">
          <input className="grow" value={directory} readOnly title={directory} />
          <button type="button" onClick={() => void choose()}>
            {t("settings.logChoose")}
          </button>
          <button type="button" onClick={() => revealItemInDir(directory).catch(console.error)} disabled={!directory}>
            {t("settings.logShow")}
          </button>
        </div>
      </div>
      {settings.directory && (
        <button type="button" className="link" onClick={() => onChange({ directory: "" })}>
          {t("settings.logDefaultDirectory")}
        </button>
      )}
      <label>
        {t("settings.logFileName")}
        <input
          value={fileName}
          onChange={(e) => setFileName(e.target.value)}
          onBlur={commitFileName}
          onKeyDown={(e) => e.key === "Enter" && commitFileName()}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </label>
      <p className="hint">{t("settings.logFileNameHint")}</p>
      <div className="row">
        <label className="grow">
          {t("settings.logFormat")}
          <select value={settings.format} onChange={(e) => onChange({ format: e.target.value as LogFormat })}>
            {LOG_FORMATS.map((format) => (
              <option key={format} value={format}>
                {t(`settings.logFormats.${format}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="grow">
          {t("settings.logKeep")}
          <select value={settings.keepDays} onChange={(e) => onChange({ keepDays: Number(e.target.value) })}>
            {/* A value set by hand in the file stays selectable. */}
            {[...new Set([...KEEP_DAYS, settings.keepDays])].map((days) => (
              <option key={days} value={days}>
                {days === 0 ? t("settings.logKeepForever") : t("settings.logKeepDays", { count: days })}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className="checkbox">
        <input
          type="checkbox"
          checked={settings.timestamps}
          disabled={settings.format === "raw"}
          onChange={(e) => onChange({ timestamps: e.target.checked })}
        />
        {t("settings.logTimestamps")}
      </label>
      {localAllowed && (
        <label className="checkbox">
          <input type="checkbox" checked={settings.autoLocal} onChange={(e) => onChange({ autoLocal: e.target.checked })} />
          {t("settings.logAutoLocal")}
        </label>
      )}
      <p className="hint">{t("settings.logAutoHint")}</p>
      <div className="row log-summary">
        <span className="grow">
          {summary && t("settings.logSummary", { count: summary.count, size: formatSize(summary.bytes) })}
        </span>
        <button
          type="button"
          className="danger"
          disabled={!summary || summary.count === 0}
          onClick={deleteAll}
          onBlur={() => setConfirmingDelete(false)}
        >
          {confirmingDelete ? t("settings.logDeleteAllConfirm", { count: summary?.count ?? 0 }) : t("settings.logDeleteAll")}
        </button>
      </div>
    </section>
  );
}

/** The app's name and version, the privacy policy and the third-party licenses. */
function AboutSection({ onShowLicenses }: { onShowLicenses(): void }) {
  const { t } = useTranslation();
  const [app, setApp] = useState<{ name: string; version: string } | null>(null);
  useEffect(() => {
    Promise.all([getName(), getVersion()])
      .then(([name, version]) => setApp({ name, version }))
      .catch(console.error);
  }, []);

  return (
    <section>
      <h3>{t("settings.about")}</h3>
      {app && (
        <div className="about-app">
          <strong>{app.name}</strong>
          <span>{t("settings.aboutVersion", { version: app.version })}</span>
        </div>
      )}
      <p className="hint">{t("settings.copyright")}</p>
      <div className="row">
        <button type="button" onClick={() => void openUrl(PRIVACY_POLICY_URL).catch(console.error)}>
          {t("settings.privacyPolicy")}
        </button>
        <button type="button" onClick={onShowLicenses}>
          {t("settings.thirdPartyLicenses")}
        </button>
      </div>
    </section>
  );
}
