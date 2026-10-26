import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useConfirmButton } from "../hooks/confirm";
import { useDialog } from "../hooks/dialogs";
import { chosenAction, matches } from "../lib/keymap";
import { isMac } from "../lib/platform";
import { CLIPBOARD_LIMIT_MAX, DEFAULT_SETTINGS } from "../ipc/api";
import {
  useSettings,
  type FileSettings,
  type LogSettings,
  type PowerSettings,
  type RightClick,
  type SidebarSettings,
  type TabSettings,
  type TerminalSettings,
  type ZmodemReceive,
  type ZmodemSettings,
} from "../hooks/settings";
import { useShortcuts } from "../hooks/shortcuts";
import { useUpdates } from "../hooks/updates";
import { useLatest } from "../hooks/latest";
import { useLocale } from "../hooks/locale";
import { CloseIcon, DotIcon } from "./icons";
import { HelpTip } from "./HelpTip";
import { LicensesDialog } from "./LicensesDialog";
import { Modal } from "./Modal";
import { RadioGroup } from "./RadioGroup";
import { Select } from "./Select";
import { AboutSection } from "./settings/AboutSection";
import { AppearanceSection } from "./settings/AppearanceSection";
import { TerminalSection } from "./settings/TerminalSection";
import { filterSettings, NumberField, Section, SECTION_REACHED, SECTIONS, Setting, type SectionId } from "./settings/common";
import { FileSection } from "./settings/FileSection";
import { KnownHostsSection } from "./settings/KnownHostsSection";
import { LogSection, TIME_FORMAT_FIELD } from "./settings/LogSection";
import { PowerSection } from "./settings/PowerSection";
import { ProxySection } from "./settings/ProxySection";
import { SecuritySection } from "./settings/SecuritySection";
import { Field } from "./Field";

interface Props {
  /** Whether the system allows local terminals (not Windows in S mode). */
  localAllowed: boolean;
  /** What opened the settings, with the section to show; a new one shows its section too. */
  request: { section?: SectionId };
  onClose(): void;
}

const ZMODEM_RECEIVE: ZmodemReceive[] = ["ask", "downloads", "chooseFolder"];
const RIGHT_CLICKS: RightClick[] = ["menu", "paste"];

/** A place in the content: a section, and how far its top is scrolled past. */
interface Place {
  section: SectionId;
  offset: number;
}

/** Where the content was scrolled to when the settings were last closed, while the app runs (in this window). */
let lastPlace: Place | null = null;

export function SettingsDialog({ localAllowed, request, onClose }: Props) {
  const { t } = useTranslation();
  const { shortcutRows } = useLocale();
  const { settings, update } = useSettings();
  const terminal = settings.terminal;
  const setTerminal = (patch: Partial<TerminalSettings>, delay?: number) =>
    update((settings) => ({ ...settings, terminal: { ...settings.terminal, ...patch } }), delay);
  const setTabs = (patch: Partial<TabSettings>) => update((settings) => ({ ...settings, tabs: { ...settings.tabs, ...patch } }));
  const setSidebar = (patch: Partial<SidebarSettings>) =>
    update((settings) => ({ ...settings, sidebar: { ...settings.sidebar, ...patch } }));
  const setPower = (patch: Partial<PowerSettings>) => update((settings) => ({ ...settings, power: { ...settings.power, ...patch } }));
  const setFiles = (patch: Partial<FileSettings>) => update((settings) => ({ ...settings, files: { ...settings.files, ...patch } }));
  const setZmodem = (patch: Partial<ZmodemSettings>) => update((settings) => ({ ...settings, zmodem: { ...settings.zmodem, ...patch } }));
  const setLogs = (patch: Partial<LogSettings>) => update((settings) => ({ ...settings, logs: { ...settings.logs, ...patch } }));
  const [licensesOpen, setLicensesOpen] = useState(false);
  const updates = useUpdates();
  const { ref: resetRef, armed: resetArmed, setArmed: setResetArmed, onBlur: onResetBlur } = useConfirmButton();
  const [query, setQuery] = useState("");
  const queryRef = useLatest(query);
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
  // The place the content was opened at, kept there while settings that load later (known hosts,
  // the version) move it, until the user scrolls.
  const restoring = useRef<Place | null>(null);

  // Focus starts in the content, so the keyboard scrolls it and never reaches a terminal.
  useEffect(() => contentRef.current?.focus({ preventScroll: true }), []);

  // In the search box, Escape clears the search first.
  const dialog = useDialog(onClose, {
    dismissable: true,
    onEscape: (e) => {
      if (e.target !== searchRef.current || !queryRef.current) onClose();
      else setQuery("");
    },
  });

  // The key that opens the settings closes them, but not the menu item, which opens them
  // wherever it is chosen. On macOS the menu bar takes ⌘, and chooses its item, so there the
  // two are one. Ctrl+F also finds outside macOS: no shell has the focus here.
  useShortcuts(
    (e) => {
      if (matches(e, "settings") && (isMac || !chosenAction(e))) {
        onClose();
        return true;
      }
      const ctrlF = !isMac && e.code === "KeyF" && e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey;
      if (!matches(e, "find") && !ctrlF) return false;
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
  }, [queryRef]);

  const restore = useCallback(() => {
    const content = contentRef.current!;
    const place = restoring.current;
    const section = place && content.querySelector<HTMLElement>(`section[data-section="${place.section}"]`);
    if (!section) return;
    content.scrollTop = section.offsetTop - parseFloat(getComputedStyle(content).paddingTop) + place.offset;
    scrollBeforeSearch.current = content.scrollTop;
    updateActive();
  }, [updateActive]);

  // Settings that appear later (the proxies once loaded, a "use the default" link) are filtered too.
  useEffect(() => {
    const observer = new MutationObserver(() => {
      applySearch();
      if (!queryRef.current) restore();
    });
    observer.observe(contentRef.current!, { childList: true, subtree: true, characterData: true });
    return () => observer.disconnect();
  }, [applySearch, restore]);

  useLayoutEffect(() => {
    applySearch();
    const content = contentRef.current!;
    content.scrollTop = query ? 0 : scrollBeforeSearch.current;
    chosen.current = false;
    updateActive();
  }, [query, applySearch, updateActive]);

  // Opened at a section, or asked for one while open: there at once, and back there when a
  // search is cleared. Opened without one: where it was last closed.
  useLayoutEffect(() => {
    const { section } = request;
    // A search may hide it, and moves the content itself.
    if (queryRef.current) return;
    restoring.current = section ? { section, offset: 0 } : lastPlace;
    restore();
    // Only for a new request.
  }, [request]);

  /** Remembers the place scrolled to, unless it is being restored or the content is filtered. */
  const scrolled = () => {
    updateActive();
    const content = contentRef.current!;
    if (restoring.current || queryRef.current) return;
    const top = content.scrollTop + parseFloat(getComputedStyle(content).paddingTop);
    let reached: HTMLElement | undefined;
    for (const section of content.querySelectorAll<HTMLElement>("section[data-section]")) if (section.offsetTop <= top) reached = section;
    if (reached) lastPlace = { section: reached.dataset.section as SectionId, offset: top - reached.offsetTop };
  };

  const search = (next: string) => {
    if (!query) scrollBeforeSearch.current = contentRef.current!.scrollTop;
    restoring.current = null;
    setQuery(next);
  };

  /** Scrolls to a section, or to `target` in it, which then goes to the middle of the content. */
  const jump = (id: SectionId, target?: HTMLElement) => {
    const content = contentRef.current!;
    const section = content.querySelector<HTMLElement>(`section[data-section="${id}"]`);
    if (!section) return;
    chosen.current = true;
    restoring.current = null;
    setActive(id);
    let top = section.offsetTop - parseFloat(getComputedStyle(content).paddingTop);
    if (target) {
      const [shown, wanted] = [content.getBoundingClientRect(), target.getBoundingClientRect()];
      top = content.scrollTop + wanted.top - shown.top - (content.clientHeight - wanted.height) / 2;
    }
    content.scrollTo({ top, behavior: "smooth" });
  };
  // Wherever the field is in its section.
  const showTimeFormat = () => {
    if (query) search("");
    requestAnimationFrame(() => {
      const field = document.getElementById(TIME_FORMAT_FIELD);
      if (!field) return;
      jump("logs", field);
      field.focus({ preventScroll: true });
    });
  };
  const release = () => {
    chosen.current = false;
    restoring.current = null;
  };

  const reset = () => {
    if (!resetArmed) {
      setResetArmed(true);
      return;
    }
    setResetArmed(false);
    // Except the language: whoever chose one may not read the default.
    update((settings) => ({ ...DEFAULT_SETTINGS, language: settings.language }));
  };

  return (
    <>
      <Modal dialog={dialog}>
        <div className="dialog settings-dialog">
          <header className="settings-header">
            <h2>{t("settings.title")}</h2>
            <button
              type="button"
              className="icon-button settings-close"
              title={t("common.close")}
              aria-label={t("common.close")}
              onClick={onClose}
            >
              <CloseIcon />
            </button>
          </header>

          <div className="settings-body">
            <nav className="settings-nav">
              <input
                ref={searchRef}
                value={query}
                placeholder={t("settings.search")}
                aria-label={t("settings.search")}
                onChange={(e) => search(e.target.value)}
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
                      {id === "about" && updates.available && (
                        <>
                          <DotIcon />
                          <span className="visually-hidden">{t("updates.available", { version: updates.available.version })}</span>
                        </>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
              <footer>
                <button type="button" ref={resetRef} onClick={reset} onBlur={onResetBlur} title={t("settings.resetHint")}>
                  {resetArmed ? t("settings.resetConfirm") : t("settings.reset")}
                </button>
              </footer>
            </nav>

            <div
              className="settings-content"
              ref={contentRef}
              tabIndex={-1}
              onScroll={scrolled}
              onWheel={release}
              onPointerDown={release}
              onKeyDown={release}
            >
              <AppearanceSection />

              <TerminalSection onShowTimeFormat={showTimeFormat} />

              <Section id="mouseAndClipboard">
                <Setting>
                  <Field group title={t("settings.rightClick")} hint={t("settings.rightClickHint")}>
                    <RadioGroup
                      className="segmented"
                      value={terminal.rightClick}
                      options={RIGHT_CLICKS.map((rightClick) => ({ value: rightClick, content: t(`settings.rightClicks.${rightClick}`) }))}
                      onChange={(rightClick) => setTerminal({ rightClick })}
                    />
                  </Field>
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
                <Setting>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={terminal.clipboardWrite}
                      onChange={(e) => setTerminal({ clipboardWrite: e.target.checked })}
                    />
                    {t("settings.clipboardWrite")}
                    <HelpTip text={t("settings.clipboardWriteTip")} />
                  </label>
                  {terminal.clipboardWrite && (
                    <Field
                      className="clipboard-limit"
                      title={
                        <>
                          {t("settings.clipboardLimit")}
                          <HelpTip text={t("settings.clipboardLimitTip")} />
                        </>
                      }
                    >
                      <NumberField
                        value={terminal.clipboardLimit}
                        min={1}
                        max={CLIPBOARD_LIMIT_MAX}
                        step={256}
                        onChange={(clipboardLimit) => setTerminal({ clipboardLimit })}
                      />
                    </Field>
                  )}
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
                <Setting>
                  <label className="checkbox">
                    <input type="checkbox" checked={settings.tabs.markOutput} onChange={(e) => setTabs({ markOutput: e.target.checked })} />
                    {t("settings.markOutput")}
                  </label>
                  <p className="hint">{t("settings.markOutputHint")}</p>
                </Setting>
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

              <PowerSection settings={settings.power} onChange={setPower} />

              <FileSection settings={settings.files} onChange={setFiles} />

              <ProxySection />

              <KnownHostsSection />

              <SecuritySection />

              <Section id="zmodem">
                <Setting>
                  <Field title={t("settings.zmodemReceive")} hint={t("settings.zmodemReceiveHint")}>
                    <Select
                      value={settings.zmodem.receive}
                      onChange={(receive) => setZmodem({ receive })}
                      options={ZMODEM_RECEIVE.map((receive) => ({ value: receive, label: t(`settings.zmodemReceiveOptions.${receive}`) }))}
                    />
                  </Field>
                </Setting>
              </Section>

              <LogSection settings={settings.logs} localAllowed={localAllowed} onChange={setLogs} />

              <Section id="shortcuts">
                {shortcutRows().map(
                  (shortcut) =>
                    (localAllowed || !shortcut.local) && (
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
          </div>
        </div>
      </Modal>
      {licensesOpen && <LicensesDialog onClose={() => setLicensesOpen(false)} />}
    </>
  );
}
