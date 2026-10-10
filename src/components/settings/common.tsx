import { useEffect, useRef, useState, type InputHTMLAttributes, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { wholeNumber } from "../../lib/format";
import { isComposing } from "../../lib/platform";
import { SpinInput } from "../SpinInput";


/** The sections in the order they appear, for the navigation; each is titled `settings.<id>`. */
export const SECTIONS = [
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
export type SectionId = (typeof SECTIONS)[number];


/** How far below the top of the content a section's title counts as scrolled to. */
export const SECTION_REACHED = 24;


/** A section of the settings, which the navigation scrolls to. */
export function Section({ id, children }: { id: SectionId; children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <section data-section={id}>
      <h3>{t(`settings.${id}`)}</h3>
      {children}
    </section>
  );
}

/** Elements that search shows or hides together, such as a checkbox and its hint. */
export function Setting({ children }: { children: ReactNode }) {
  return <div className="setting">{children}</div>;
}

/**
 * Hides the settings (a section's children, see `Setting`) that don't match every word of
 * `query`, each word found in the setting's text (label, hint, choices) or its section's
 * title, and the sections left without any. Returns the sections still shown. It reads the
 * rendered text, so that it covers every setting in any language without a list to keep in
 * sync.
 */
export function filterSettings(content: HTMLElement, query: string): SectionId[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown: SectionId[] = [];
  for (const section of Array.from(content.querySelectorAll<HTMLElement>("section[data-section]"))) {
    const heading = section.querySelector("h3");
    const title = heading?.textContent?.toLowerCase() ?? "";
    let any = false;
    for (const item of Array.from(section.children)) {
      if (item === heading || !(item instanceof HTMLElement)) continue;
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
export function NumberField({
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
    const n = wholeNumber(next);
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
export function LiveTextField({
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


/**
 * A folder setting: the folder in effect (read only), and buttons to choose another (if
 * `onChoose`), show it in the file manager, and go back to the default (if `reset`).
 */
export function FolderField({
  label,
  path,
  onChoose,
  onShow,
  reset,
}: {
  label: string;
  path: string;
  onChoose?(): void;
  /** Defaults to revealing `path`. */
  onShow?(): void;
  reset?: { label: string; onReset(): void } | null;
}) {
  const { t } = useTranslation();
  return (
    <>
      <div className="field">
        <span>{label}</span>
        <div className="row">
          <input className="grow" value={path} readOnly title={path} aria-label={label} />
          {onChoose && (
            <button type="button" onClick={onChoose}>
              {t("settings.logChoose")}
            </button>
          )}
          <button type="button" onClick={onShow ?? (() => void revealItemInDir(path).catch(console.error))} disabled={!path}>
            {t("settings.logShow")}
          </button>
        </div>
      </div>
      {reset && (
        <button type="button" className="link" onClick={reset.onReset}>
          {reset.label}
        </button>
      )}
    </>
  );
}
