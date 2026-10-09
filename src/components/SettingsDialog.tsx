import { useCallback, useEffect, useLayoutEffect, useRef, useState, type InputHTMLAttributes, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getName, getVersion } from "@tauri-apps/api/app";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

import { configDirectory, knownHosts, logs, proxies as proxyApi, sftp, type Proxy } from "../lib/api";
import { useConfirmButton } from "../lib/confirm";
import { basename, formatSize } from "../lib/format";
import {
  allCopyShortcutsLabel,
  allPasteShortcutsLabel,
  closeTabShortcutLabel,
  closeWindowShortcutLabel,
  findShortcutLabel,
  goToTabShortcutLabel,
  isComposing,
  isFindShortcut,
  isMac,
  lastTabShortcutLabel,
  newTabShortcutLabel,
  nextTabShortcutLabel,
  paneFocusShortcutLabel,
  previousTabShortcutLabel,
  searchShortcutLabel,
  settingsShortcutLabel,
  shiftShortcutLabel,
  splitDownShortcutLabel,
  splitRightShortcutLabel,
} from "../lib/platform";
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
  type TextSize,
  type ZmodemSettings,
} from "../lib/settings";
import { DEFAULT_FONT_STACK, TERMINAL_SCHEMES, resolveScheme, type TerminalScheme } from "../lib/terminalSchemes";
import { HelpTip } from "./HelpTip";
import { CloseIcon } from "./icons";
import { KnownHostsDialog } from "./KnownHostsDialog";
import { LicensesDialog } from "./LicensesDialog";
import { ProxyDialog, proxySummary } from "./ProxyDialog";
import { SchemePreview, schemeLabel } from "./SchemePreview";
import { SpinInput } from "./SpinInput";

interface Props {
  /** Whether the system allows local terminals (not Windows in S mode). */
  localAllowed: boolean;
  onClose(): void;
}

const APPEARANCES: Appearance[] = ["system", "dark", "light"];
const TEXT_SIZES: TextSize[] = ["normal", "large", "larger"];
const CURSOR_STYLES: CursorStyle[] = ["block", "bar", "underline"];
const RIGHT_CLICKS: RightClick[] = ["menu", "paste"];
const LOG_FORMATS: LogFormat[] = ["text", "raw"];
/** Retention choices, in days; 0 keeps logs. */
const KEEP_DAYS = [0, 7, 30, 90, 365];
const PRIVACY_POLICY_URL = "https://github.com/Bokjan/ZShell/blob/main/PRIVACY.md";

/** The sections in the order they appear, for the navigation; each is titled `settings.<id>`. */
const SECTIONS = [
  "appearance",
  "terminal",
  "mouseAndClipboard",
  "tabs",
  "sidebar",
  "files",
  "proxies",
  "knownHosts",
  "zmodem",
  "logs",
  "shortcuts",
  "about",
] as const;
type SectionId = (typeof SECTIONS)[number];

/**
 * The app's shortcuts on this platform, each described by `settings.shortcutActions.<id>`;
 * those without keys here (closing the window on Windows, plain Ctrl+C on macOS) are left out.
 */
const SHORTCUTS = [
  { id: "searchSessions", keys: searchShortcutLabel },
  { id: "newLocalTerminal", keys: newTabShortcutLabel, local: true },
  { id: "nextTab", keys: nextTabShortcutLabel },
  { id: "previousTab", keys: previousTabShortcutLabel },
  { id: "goToTab", keys: goToTabShortcutLabel },
  { id: "lastTab", keys: lastTabShortcutLabel },
  { id: "closeTab", keys: closeTabShortcutLabel },
  { id: "closeWindow", keys: closeWindowShortcutLabel },
  { id: "splitRight", keys: splitRightShortcutLabel },
  { id: "splitDown", keys: splitDownShortcutLabel },
  { id: "focusPane", keys: paneFocusShortcutLabel },
  { id: "copy", keys: allCopyShortcutsLabel },
  { id: "copySelection", keys: isMac ? undefined : "Ctrl+C" },
  { id: "paste", keys: allPasteShortcutsLabel },
  { id: "find", keys: findShortcutLabel },
  { id: "composeBar", keys: shiftShortcutLabel("I") },
  { id: "quickCommands", keys: shiftShortcutLabel("J") },
  { id: "filePanel", keys: shiftShortcutLabel("E") },
  { id: "forwardsPanel", keys: shiftShortcutLabel("P") },
  { id: "settings", keys: settingsShortcutLabel },
] as const;

/** How far below the top of the content a section's title counts as scrolled to. */
const SECTION_REACHED = 24;

/** A section of the settings, which the navigation scrolls to. */
function Section({ id, children }: { id: SectionId; children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <section data-section={id}>
      <h3>{t(`settings.${id}`)}</h3>
      {children}
    </section>
  );
}

/** Elements that search shows or hides together, such as a checkbox and its hint. */
function Setting({ children }: { children: ReactNode }) {
  return <div className="setting">{children}</div>;
}

/**
 * Hides the settings (a section's children, see `Setting`) that don't match every word of
 * `query`, each word found in the setting's text (label, hint, choices) or its section's
 * title, and the sections left without any. Returns the sections still shown. It reads the
 * rendered text, so that it covers every setting in any language without a list to keep in
 * sync.
 */
function filterSettings(content: HTMLElement, query: string): SectionId[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown: SectionId[] = [];
  for (const section of Array.from(content.querySelectorAll<HTMLElement>("section[data-section]"))) {
    const heading = section.querySelector("h3");
    const title = heading?.textContent?.toLowerCase() ?? "";
    let any = false;
    for (const item of Array.from(section.children)) {
      // Dialogs opened from a section (a proxy, the known hosts) are rendered inside it.
      if (item === heading || !(item instanceof HTMLElement) || item.classList.contains("dialog-backdrop")) continue;
      const text = item.textContent?.toLowerCase() ?? "";
      item.hidden = !words.every((word) => title.includes(word) || text.includes(word));
      any ||= !item.hidden;
    }
    section.hidden = !any;
    if (any) shown.push(section.dataset.section as SectionId);
  }
  return shown;
}

/**
 * A number field that only reports values that are integers within range. Typed values take
 * effect when the field loses the focus, on Enter or when the dialog closes, not on each
 * key: on the way to 20000, scrollback would be cut to 2 lines (dropping every terminal's
 * history) and the font set to size 1. Stepping takes effect at once.
 */
function NumberField({
  value,
  min,
  max,
  step,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange(n: number): void;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const valid = (n: number) => Number.isInteger(n) && n >= min && n <= max;
  const commit = (next: string) => {
    const n = Number(next);
    if (next.trim() !== "" && valid(n) && n !== value) onChange(n);
    else setText(String(value));
  };
  const commitLatest = useRef(() => {});
  commitLatest.current = () => commit(text);
  useEffect(() => () => commitLatest.current(), []);
  return (
    <SpinInput
      value={text}
      min={min}
      max={max}
      step={step}
      start={value}
      aria-invalid={!valid(Number(text))}
      onChange={(next, stepped) => {
        setText(next);
        if (stepped) commit(next);
      }}
      onBlur={() => commit(text)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !isComposing(e)) commit(text);
      }}
    />
  );
}

/**
 * A text field applied as it is typed. The stored value comes back trimmed, which must not
 * replace what is being typed: a space typed before the next word would disappear.
 */
function LiveTextField({
  value,
  onChange,
  ...rest
}: { value: string; onChange(text: string): void } & Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange">) {
  const [text, setText] = useState(value);
  useEffect(() => setText((current) => (current.trim() === value ? current : value)), [value]);
  return (
    <input
      {...rest}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        onChange(e.target.value);
      }}
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
  const resetButton = useConfirmButton();
  const [query, setQuery] = useState("");
  const queryRef = useRef(query);
  queryRef.current = query;
  // The sections with settings matching the search: all of them without one.
  const [shown, setShown] = useState<readonly SectionId[]>(SECTIONS);
  // The section scrolled to, highlighted in the navigation; none when the search finds nothing.
  const [active, setActive] = useState<SectionId | null>(SECTIONS[0]);
  const contentRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  // Set when a section is chosen in the navigation: it stays highlighted while the content
  // scrolls to it, and when it is too near the end to reach the top, until the user scrolls.
  const chosen = useRef(false);
  // Where the content was scrolled to before searching; clearing the search goes back there.
  const scrollBeforeSearch = useRef(0);

  // Focus starts in the content, so the keyboard scrolls it and never reaches a terminal.
  useEffect(() => contentRef.current?.focus({ preventScroll: true }), []);

  // While the licenses are open, Escape closes only them (a proxy dialog stops it itself).
  // In the search box, it clears the search first. Ctrl+F also finds outside macOS: no shell
  // has the focus here.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (licensesOpen || isComposing(e)) return;
      if (e.key === "Escape") {
        if (e.target !== searchRef.current || !queryRef.current) onClose();
        else setQuery("");
      } else if (isFindShortcut(e) || (!isMac && e.code === "KeyF" && e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey)) {
        e.preventDefault();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, licensesOpen]);

  const updateActive = useCallback(() => {
    const content = contentRef.current;
    if (!content || chosen.current) return;
    const sections = Array.from(content.querySelectorAll<HTMLElement>("section[data-section]:not([hidden])"));
    if (sections.length === 0) {
      setActive(null);
      return;
    }
    const top = content.scrollTop + parseFloat(getComputedStyle(content).paddingTop) + SECTION_REACHED;
    let reached = sections[0];
    for (const section of sections) if (section.offsetTop <= top) reached = section;
    // At the end, the last section counts as reached even when it is too short to reach the top.
    if (content.scrollTop + content.clientHeight >= content.scrollHeight - 1) reached = sections[sections.length - 1];
    setActive(reached.dataset.section as SectionId);
  }, []);

  const applySearch = useCallback(() => {
    const next = filterSettings(contentRef.current!, queryRef.current);
    setShown((prev) => (prev.join() === next.join() ? prev : next));
  }, []);

  // Settings that appear later (the proxies once loaded, a "use the default" link) are filtered too.
  useEffect(() => {
    const observer = new MutationObserver(applySearch);
    observer.observe(contentRef.current!, { childList: true, subtree: true, characterData: true });
    return () => observer.disconnect();
  }, [applySearch]);

  useLayoutEffect(() => {
    applySearch();
    const content = contentRef.current!;
    content.scrollTop = query ? 0 : scrollBeforeSearch.current;
    chosen.current = false;
    updateActive();
  }, [query, applySearch, updateActive]);

  const search = (next: string) => {
    if (!query) scrollBeforeSearch.current = contentRef.current!.scrollTop;
    setQuery(next);
  };

  const jump = (id: SectionId) => {
    const content = contentRef.current!;
    const section = content.querySelector<HTMLElement>(`section[data-section="${id}"]`);
    if (!section) return;
    chosen.current = true;
    setActive(id);
    content.scrollTo({ top: section.offsetTop - parseFloat(getComputedStyle(content).paddingTop), behavior: "smooth" });
  };
  const release = () => {
    chosen.current = false;
  };

  const reset = () => {
    if (!resetButton.armed) {
      resetButton.setArmed(true);
      return;
    }
    resetButton.setArmed(false);
    update(DEFAULT_SETTINGS);
  };

  const schemeName = (scheme: TerminalScheme) => schemeLabel(scheme, t);

  return (
    <>
      <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
        <div className="dialog settings-dialog" role="dialog" aria-label={t("settings.title")}>
          <nav className="settings-nav">
            <h2>{t("settings.title")}</h2>
            <input
              ref={searchRef}
              value={query}
              placeholder={t("settings.search")}
              aria-label={t("settings.search")}
              onChange={(e) => search(e.target.value)}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
            />
            <ul className="settings-nav-list">
              {SECTIONS.map((id) => (
                <li key={id}>
                  <button
                    type="button"
                    className={id === active ? "on" : undefined}
                    aria-current={id === active ? "true" : undefined}
                    disabled={!shown.includes(id)}
                    onClick={() => jump(id)}
                  >
                    {t(`settings.${id}`)}
                  </button>
                </li>
              ))}
            </ul>
            <footer>
              <button type="button" ref={resetButton.ref} onClick={reset} onBlur={resetButton.onBlur}>
                {resetButton.armed ? t("settings.resetConfirm") : t("settings.reset")}
              </button>
            </footer>
          </nav>

          <div
            className="settings-content"
            ref={contentRef}
            tabIndex={-1}
            onScroll={updateActive}
            onWheel={release}
            onPointerDown={release}
            onKeyDown={release}
          >
            <Section id="appearance">
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
              <Setting>
                <div className="field">
                  <span>{t("settings.textSize")}</span>
                  <div className="segmented" role="radiogroup">
                    {TEXT_SIZES.map((textSize) => (
                      <button
                        key={textSize}
                        role="radio"
                        aria-checked={settings.textSize === textSize}
                        className={settings.textSize === textSize ? "on" : undefined}
                        onClick={() => update({ ...settings, textSize })}
                      >
                        {t(`settings.textSizes.${textSize}`)}
                      </button>
                    ))}
                  </div>
                </div>
                <p className="hint">{t("settings.textSizeHint")}</p>
              </Setting>
            </Section>

            <Section id="terminal">
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

              <Setting>
                <div className="row">
                  <label className="grow">
                    {t("settings.fontFamily")}
                    <LiveTextField
                      value={terminal.fontFamily}
                      onChange={(fontFamily) => setTerminal({ fontFamily })}
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
              </Setting>

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
                    step={1000}
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
            </Section>

            <Section id="mouseAndClipboard">
              <Setting>
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
              </Setting>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={terminal.copyOnSelect}
                  onChange={(e) => setTerminal({ copyOnSelect: e.target.checked })}
                />
                {t("settings.copyOnSelect")}
              </label>
              <Setting>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={terminal.confirmMultilinePaste}
                    onChange={(e) => setTerminal({ confirmMultilinePaste: e.target.checked })}
                  />
                  {t("settings.confirmMultilinePaste")}
                </label>
                <p className="hint">{t("settings.confirmMultilinePasteHint")}</p>
              </Setting>
              {isMac && (
                <Setting>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={terminal.optionAsMeta}
                      onChange={(e) => setTerminal({ optionAsMeta: e.target.checked })}
                    />
                    {t("settings.optionAsMeta")}
                  </label>
                  <p className="hint">{t("settings.optionAsMetaHint")}</p>
                </Setting>
              )}
            </Section>

            <Section id="tabs">
              <Setting>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={settings.tabs.followRemoteTitle}
                    onChange={(e) => setTabs({ followRemoteTitle: e.target.checked })}
                  />
                  {t("settings.followRemoteTitle")}
                </label>
                <p className="hint">{t("settings.followRemoteTitleHint")}</p>
              </Setting>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={settings.tabs.confirmClose}
                  onChange={(e) => setTabs({ confirmClose: e.target.checked })}
                />
                {t("settings.confirmClose")}
              </label>
            </Section>

            <Section id="sidebar">
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={settings.sidebar.showRecent}
                  onChange={(e) => setSidebar({ showRecent: e.target.checked })}
                />
                {t("settings.showRecent")}
              </label>
            </Section>

            <FileSection settings={settings.files} onChange={setFiles} />

            <ProxySection />

            <KnownHostsSection />

            <Section id="zmodem">
              <Setting>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={settings.zmodem.askDownloadLocation}
                    onChange={(e) => setZmodem({ askDownloadLocation: e.target.checked })}
                  />
                  {t("settings.zmodemAskLocation")}
                </label>
                <p className="hint">{t("settings.zmodemAskLocationHint")}</p>
              </Setting>
            </Section>

            <LogSection settings={settings.logs} localAllowed={localAllowed} onChange={setLogs} />

            <Section id="shortcuts">
              {SHORTCUTS.map(
                (shortcut) =>
                  shortcut.keys &&
                  (localAllowed || !("local" in shortcut)) && (
                    <div key={shortcut.id} className="shortcut">
                      <span>{t(`settings.shortcutActions.${shortcut.id}`)}</span>
                      <kbd>{shortcut.keys}</kbd>
                    </div>
                  ),
              )}
            </Section>

            <AboutSection onShowLicenses={() => setLicensesOpen(true)} />

            {shown.length === 0 && <p className="settings-empty">{t("settings.noMatches")}</p>}
          </div>

          <button
            type="button"
            className="icon-button settings-close"
            title={t("common.close")}
            aria-label={t("common.close")}
            onClick={onClose}
          >
            <CloseIcon />
          </button>
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
    <Section id="files">
      <Setting>
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
      </Setting>
      <Setting>
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
      </Setting>
    </Section>
  );
}

/** The saved proxies, which sessions choose on their Connection page. */
function ProxySection() {
  const { t } = useTranslation();
  const [list, setList] = useState<Proxy[]>([]);
  // The proxy being edited; null for a new one.
  const [editing, setEditing] = useState<Proxy | null | undefined>(undefined);

  const refresh = useCallback(() => {
    proxyApi.list().then(setList).catch(console.error);
  }, []);
  useEffect(refresh, [refresh]);

  return (
    <Section id="proxies">
      <Setting>
        {list.length > 0 && (
          <ul className="proxy-list">
            {list.map((proxy) => (
              <li key={proxy.id}>
                <button type="button" onClick={() => setEditing(proxy)}>
                  <span className="proxy-name">{proxy.name}</span>
                  <span className="proxy-summary">{proxySummary(t, proxy)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="row">
          <button type="button" onClick={() => setEditing(null)}>
            {t("settings.addProxy")}
          </button>
        </div>
        <p className="hint">{t("settings.proxiesHint")}</p>
      </Setting>
      {editing !== undefined && (
        <ProxyDialog proxy={editing} onClose={() => setEditing(undefined)} onChanged={refresh} />
      )}
    </Section>
  );
}

/** The host keys in `~/.ssh/known_hosts`, managed in their own dialog. */
function KnownHostsSection() {
  const { t } = useTranslation();
  const [path, setPath] = useState<string | null>(null);
  const [count, setCount] = useState<number | null>(null);
  const [managing, setManaging] = useState(false);

  const refresh = useCallback(() => {
    knownHosts.path().then(setPath).catch(console.error);
    knownHosts.list().then((entries) => setCount(entries.length), console.error);
  }, []);
  useEffect(refresh, [refresh]);

  return (
    <Section id="knownHosts">
      <Setting>
        {count !== null && (
          <p className="known-hosts-summary">
            {t("settings.knownHostsCount", { count, file: "~/.ssh/known_hosts" })}
            {path && <HelpTip text={path} />}
          </p>
        )}
        <div className="row">
          <button type="button" onClick={() => setManaging(true)}>
            {t("settings.knownHostsManage")}
          </button>
          <button
            type="button"
            disabled={!path || !count}
            onClick={() => path && void revealItemInDir(path).catch(console.error)}
          >
            {t(isMac ? "settings.knownHostsReveal" : "settings.knownHostsRevealWindows")}
          </button>
        </div>
        <p className="hint">{t("settings.knownHostsHint")}</p>
      </Setting>
      {managing && (
        <KnownHostsDialog
          onClose={() => {
            setManaging(false);
            refresh();
          }}
          onChanged={refresh}
        />
      )}
    </Section>
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
    <Section id="logs">
      <Setting>
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
      </Setting>
      <Setting>
        <label>
          {t("settings.logFileName")}
          <input
            value={fileName}
            onChange={(e) => setFileName(e.target.value)}
            onBlur={commitFileName}
            onKeyDown={(e) => e.key === "Enter" && !isComposing(e) && commitFileName()}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <p className="hint">{t("settings.logFileNameHint")}</p>
      </Setting>
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
      <Setting>
        {localAllowed && (
          <label className="checkbox">
            <input type="checkbox" checked={settings.autoLocal} onChange={(e) => onChange({ autoLocal: e.target.checked })} />
            {t("settings.logAutoLocal")}
          </label>
        )}
        <p className="hint">{t("settings.logAutoHint")}</p>
      </Setting>
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
    </Section>
  );
}

/** The app's name and version, where its data is, the privacy policy and the third-party licenses. */
function AboutSection({ onShowLicenses }: { onShowLicenses(): void }) {
  const { t } = useTranslation();
  const [app, setApp] = useState<{ name: string; version: string } | null>(null);
  const [directory, setDirectory] = useState("");
  useEffect(() => {
    Promise.all([getName(), getVersion()])
      .then(([name, version]) => setApp({ name, version }))
      .catch(console.error);
    configDirectory().then(setDirectory).catch(console.error);
  }, []);

  return (
    <Section id="about">
      <Setting>
        {app && (
          <div className="about-app">
            <strong>{app.name}</strong>
            <span>{t("settings.aboutVersion", { version: app.version })}</span>
          </div>
        )}
        <p className="hint">{t("settings.copyright")}</p>
        <div className="about-links">
          <button type="button" className="link" onClick={() => void openUrl(PRIVACY_POLICY_URL).catch(console.error)}>
            {t("settings.privacyPolicy")}
          </button>
          <button type="button" className="link" onClick={onShowLicenses}>
            {t("settings.thirdPartyLicenses")}
          </button>
        </div>
      </Setting>
      <Setting>
        <div className="field">
          <span>{t("settings.dataFolder")}</span>
          <div className="row">
            <input className="grow" value={directory} readOnly title={directory} />
            <button type="button" onClick={() => revealItemInDir(directory).catch(console.error)} disabled={!directory}>
              {t("settings.logShow")}
            </button>
          </div>
        </div>
        <p className="hint">{t(isMac ? "settings.dataFolderHint" : "settings.dataFolderHintWindows")}</p>
      </Setting>
    </Section>
  );
}
