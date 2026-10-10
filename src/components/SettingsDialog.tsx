import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useConfirmButton } from "../lib/confirm";
import { useDialog } from "../lib/dialogs";
import {
  allCopyShortcutsLabel,
  allPasteShortcutsLabel,
  closeTabShortcutLabel,
  closeWindowShortcutLabel,
  findShortcutLabel,
  goToTabShortcutLabel,
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
  type LogSettings,
  type RightClick,
  type SidebarSettings,
  type TabSettings,
  type TerminalSettings,
  type TextSize,
  type ZmodemReceive,
  type ZmodemSettings,
} from "../lib/settings";
import { useShortcuts } from "../lib/shortcuts";
import { DEFAULT_FONT_STACK, TERMINAL_SCHEMES, resolveScheme, type TerminalScheme } from "../lib/terminalSchemes";
import { CloseIcon } from "./icons";
import { LicensesDialog } from "./LicensesDialog";
import { Modal } from "./Modal";
import { RadioGroup } from "./RadioGroup";
import { SchemePreview, schemeLabel } from "./SchemePreview";
import { AboutSection } from "./settings/AboutSection";
import {
  filterSettings,
  LiveTextField,
  NumberField,
  Section,
  SECTION_REACHED,
  SECTIONS,
  Setting,
  type SectionId,
} from "./settings/common";
import { FileSection } from "./settings/FileSection";
import { KnownHostsSection } from "./settings/KnownHostsSection";
import { LogSection } from "./settings/LogSection";
import { ProxySection } from "./settings/ProxySection";

interface Props {
  /** Whether the system allows local terminals (not Windows in S mode). */
  localAllowed: boolean;
  onClose(): void;
}

const APPEARANCES: Appearance[] = ["system", "dark", "light"];
const TEXT_SIZES: TextSize[] = ["normal", "large", "larger"];
const CURSOR_STYLES: CursorStyle[] = ["block", "bar", "underline"];
const ZMODEM_RECEIVE: ZmodemReceive[] = ["ask", "downloads", "chooseFolder"];
const RIGHT_CLICKS: RightClick[] = ["menu", "paste"];

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

  // In the search box, Escape clears the search first.
  const dialog = useDialog(onClose, {
    onEscape: (e) => {
      if (e.target !== searchRef.current || !queryRef.current) onClose();
      else setQuery("");
    },
  });

  // Ctrl+F also finds outside macOS: no shell has the focus here.
  useShortcuts(
    (e) => {
      const ctrlF = !isMac && e.code === "KeyF" && e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey;
      if (!isFindShortcut(e) && !ctrlF) return false;
      searchRef.current?.select();
      return true;
    },
    { dialog },
  );

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
      <Modal dialog={dialog}>
        <div className="dialog settings-dialog">
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
              <RadioGroup
                className="segmented"
                label={t("settings.appearance")}
                value={settings.appearance}
                options={APPEARANCES.map((appearance) => ({ value: appearance, content: t(`settings.appearances.${appearance}`) }))}
                onChange={(appearance) => update({ ...settings, appearance })}
              />
              <Setting>
                <div className="field">
                  <span>{t("settings.textSize")}</span>
                  <RadioGroup
                    className="segmented"
                    label={t("settings.textSize")}
                    value={settings.textSize}
                    options={TEXT_SIZES.map((textSize) => ({ value: textSize, content: t(`settings.textSizes.${textSize}`) }))}
                    onChange={(textSize) => update({ ...settings, textSize })}
                  />
                </div>
                <p className="hint">{t("settings.textSizeHint")}</p>
              </Setting>
            </Section>

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
                          <span className="scheme-name">{schemeName(scheme)}</span>
                        </>
                      ),
                    })),
                  ]}
                  onChange={(colorScheme) => setTerminal({ colorScheme })}
                />
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
              <Setting>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={terminal.screenReader}
                    onChange={(e) => setTerminal({ screenReader: e.target.checked })}
                  />
                  {t("settings.screenReader")}
                </label>
                <p className="hint">{t("settings.screenReaderHint")}</p>
              </Setting>
            </Section>

            <Section id="mouseAndClipboard">
              <Setting>
                <div className="field">
                  <span>{t("settings.rightClick")}</span>
                  <RadioGroup
                    className="segmented"
                    label={t("settings.rightClick")}
                    value={terminal.rightClick}
                    options={RIGHT_CLICKS.map((rightClick) => ({ value: rightClick, content: t(`settings.rightClicks.${rightClick}`) }))}
                    onChange={(rightClick) => setTerminal({ rightClick })}
                  />
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
                <label>
                  {t("settings.zmodemReceive")}
                  <select
                    value={settings.zmodem.receive}
                    onChange={(e) => setZmodem({ receive: e.target.value as ZmodemReceive })}
                  >
                    {ZMODEM_RECEIVE.map((receive) => (
                      <option key={receive} value={receive}>
                        {t(`settings.zmodemReceiveOptions.${receive}`)}
                      </option>
                    ))}
                  </select>
                </label>
                <p className="hint">{t("settings.zmodemReceiveHint")}</p>
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
      </Modal>
      {licensesOpen && <LicensesDialog onClose={() => setLicensesOpen(false)} />}
    </>
  );
}
