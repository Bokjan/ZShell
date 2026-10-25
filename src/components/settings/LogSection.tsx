import { useCallback, useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { DEFAULT_SETTINGS, errorMessage, logs } from "../../ipc/api";
import { useConfirmButton } from "../../hooks/confirm";
import { open as openDialog } from "../../ipc/fileDialog";
import { isComposing } from "../../lib/platform";
import type { LogFormat, LogSettings } from "../../hooks/settings";
import { useWindowFocus } from "../../hooks/windowEvents";
import { useLatest } from "../../hooks/latest";
import { useLocale } from "../../hooks/locale";
import { formatTime } from "../../lib/terminalNotice";
import { ComboBox } from "../ComboBox";
import { Select } from "../Select";
import { FolderField, Section, Setting, useFieldText } from "./common";
import { Field } from "../Field";

const LOG_FORMATS: LogFormat[] = ["text", "raw"];
/** The time formats offered, with the key of their description. */
const TIME_FORMATS = [
  { format: "[%Y-%m-%d %H:%M:%S]", name: "dateTime" },
  { format: "[%Y-%m-%d %H:%M:%S.%3f]", name: "dateTimeMs" },
  { format: "[%H:%M:%S]", name: "time" },
  { format: "[%H:%M:%S.%3f]", name: "timeMs" },
] as const;
/** The id of the time format field, which the terminal settings point to. */
export const TIME_FORMAT_FIELD = "settings-time-format";
/** Retention choices, in days; 0 keeps logs. */
const KEEP_DAYS = [0, 7, 30, 90, 365];

/** Session logs: where and how they are written, how long they are kept, deleting them. */
export function LogSection({
  settings,
  localAllowed,
  onChange,
}: {
  settings: LogSettings;
  localAllowed: boolean;
  onChange(patch: Partial<LogSettings>): void;
}) {
  const { t } = useTranslation();
  const { formatSize } = useLocale();
  const [directory, setDirectory] = useState("");
  const [summary, setSummary] = useState<{ count: number; bytes: number } | null>(null);
  const { ref: deleteRef, armed: deleteArmed, setArmed: setDeleteArmed, onBlur: onDeleteBlur } = useConfirmButton();
  // Saved trimmed, when the field is left.
  const [fileName, setFileName] = useFieldText(settings.fileName, (text, value) => text.trim() === value);

  const refresh = useCallback(() => {
    logs.summary().then(setSummary).catch(console.error);
  }, []);
  useEffect(refresh, [refresh]);
  // Logs written or deleted meanwhile (in another window, or outside ZShell).
  useWindowFocus(refresh);
  // The folder in effect (the default one when none is chosen).
  useEffect(() => {
    logs.directory().then(setDirectory).catch(console.error);
  }, [settings.directory]);

  const choose = async () => {
    const picked = await openDialog({ directory: true, defaultPath: directory || undefined }).catch(() => null);
    if (typeof picked === "string") onChange({ directory: picked });
  };

  const deleteAll = () => {
    if (!deleteArmed) {
      setDeleteArmed(true);
      return;
    }
    setDeleteArmed(false);
    logs.delete().then(refresh, console.error);
  };

  // The file name is saved when the field is left (or the dialog closes, which may not blur
  // it first), so that clearing it to type another doesn't bring the default back in the
  // middle.
  const commitFileName = () => {
    if (fileName.trim() !== settings.fileName) onChange({ fileName: fileName.trim() });
  };
  const commitRef = useLatest(commitFileName);
  useEffect(() => () => commitRef.current(), []);

  return (
    <Section id="logs">
      <Setting>
        <FolderField
          label={t("settings.logDirectory")}
          path={directory}
          onChoose={() => void choose()}
          // Created first: it may not exist until a log is written.
          onShow={() => logs.directory(true).then(revealItemInDir).catch(console.error)}
          reset={settings.directory ? { label: t("settings.logDefaultDirectory"), onReset: () => onChange({ directory: "" }) } : null}
        />
      </Setting>
      <Setting>
        <Field title={t("settings.logFileName")} hint={t("settings.logFileNameHint")}>
          <input
            value={fileName}
            onChange={(e) => setFileName(e.target.value)}
            onBlur={commitFileName}
            onKeyDown={(e) => e.key === "Enter" && !isComposing(e) && commitFileName()}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </Field>
      </Setting>
      <div className="row">
        <Field className="grow" title={t("settings.logFormat")}>
          <Select
            value={settings.format}
            onChange={(format) => onChange({ format })}
            options={LOG_FORMATS.map((format) => ({ value: format, label: t(`settings.logFormats.${format}`) }))}
          />
        </Field>
        <Field className="grow" title={t("settings.logKeep")}>
          <Select
            value={settings.keepDays}
            onChange={(keepDays) => onChange({ keepDays })}
            // A value set by hand in the file stays selectable.
            options={[...new Set([...KEEP_DAYS, settings.keepDays])].map((days) => ({
              value: days,
              label: days === 0 ? t("settings.logKeepForever") : t("settings.logKeepDays", { count: days }),
            }))}
          />
        </Field>
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
      <TimeFormatField value={settings.timeFormat} onChange={(timeFormat) => onChange({ timeFormat })} />
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
        <span className="grow">{summary && t("settings.logSummary", { count: summary.count, size: formatSize(summary.bytes) })}</span>
        <button
          type="button"
          className="danger"
          disabled={!summary || summary.count === 0}
          ref={deleteRef}
          onClick={deleteAll}
          onBlur={onDeleteBlur}
        >
          {deleteArmed ? t("settings.logDeleteAllConfirm", { count: summary?.count ?? 0 }) : t("settings.logDeleteAll")}
        </button>
      </div>
    </Section>
  );
}

/**
 * The format of the times that start log lines and our own messages in the terminal: one of
 * those offered, or typed. The backend checks it as it is typed, with an example of the
 * current time in it, and it is saved when the field is left, if the backend took it.
 */
function TimeFormatField({ value, onChange }: { value: string; onChange(format: string): void }) {
  const { t } = useTranslation();
  const noteId = useId();
  const hintId = useId();
  const [text, setText] = useFieldText(value, (text, value) => text.trim() === value);
  // The example, or why the format can't be used.
  const [checked, setChecked] = useState<{ example: string } | { error: string } | null>(null);
  const format = text.trim() || DEFAULT_SETTINGS.logs.timeFormat;

  useEffect(() => {
    let current = true;
    logs.timeFormat(format).then(
      (parts) => current && setChecked({ example: formatTime(parts, new Date()) }),
      (e: unknown) => current && setChecked({ error: errorMessage(e) }),
    );
    return () => {
      current = false;
    };
  }, [format]);

  // Saved only once the backend has taken it; the field goes on showing why otherwise.
  const commit = (next = format) => {
    if (next === value) return;
    logs.timeFormat(next).then(() => onChange(next), ignoreInvalid);
  };
  const commitRef = useLatest(commit);
  useEffect(() => () => commitRef.current(), []);

  const invalid = checked !== null && "error" in checked;
  return (
    <Setting>
      <Field title={t("settings.logTimeFormat")}>
        <ComboBox
          id={TIME_FORMAT_FIELD}
          value={text}
          onChange={(next) => {
            setText(next);
            // Picked from the list rather than typed.
            if (TIME_FORMATS.some(({ format }) => format === next)) commit(next);
          }}
          onBlur={() => commit()}
          onKeyDown={(e) => e.key === "Enter" && !isComposing(e) && commit()}
          suggestions={TIME_FORMATS.map(({ format, name }) => ({ value: format, description: t(`settings.logTimeFormats.${name}`) }))}
          aria-invalid={invalid}
          aria-describedby={`${noteId} ${hintId}`}
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
      </Field>
      <p className={invalid ? "hint warning" : "hint"} id={noteId}>
        {checked && ("error" in checked ? checked.error : t("settings.logTimeFormatExample", { time: checked.example }))}
      </p>
      <p className="hint" id={hintId}>
        {t("settings.logTimeFormatHint")}
      </p>
    </Setting>
  );
}

/** A format the backend refused: the field already says why. */
const ignoreInvalid = () => {};
