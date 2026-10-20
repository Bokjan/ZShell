import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { logs } from "../../ipc/api";
import { useConfirmButton } from "../../hooks/confirm";
import { open as openDialog } from "../../ipc/fileDialog";
import { isComposing } from "../../lib/platform";
import type { LogFormat, LogSettings } from "../../hooks/settings";
import { useWindowFocus } from "../../hooks/windowEvents";
import { useLatest } from "../../hooks/latest";
import { useLocale } from "../../hooks/locale";
import { Select } from "../Select";
import { FolderField, Section, Setting, useFieldText } from "./common";

const LOG_FORMATS: LogFormat[] = ["text", "raw"];
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
        <label>
          {t("settings.logFileName")}
          <input
            value={fileName}
            onChange={(e) => setFileName(e.target.value)}
            onBlur={commitFileName}
            onKeyDown={(e) => e.key === "Enter" && !isComposing(e) && commitFileName()}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <p className="hint">{t("settings.logFileNameHint")}</p>
      </Setting>
      <div className="row">
        <label className="grow">
          {t("settings.logFormat")}
          <Select
            value={settings.format}
            onChange={(format) => onChange({ format })}
            options={LOG_FORMATS.map((format) => ({ value: format, label: t(`settings.logFormats.${format}`) }))}
          />
        </label>
        <label className="grow">
          {t("settings.logKeep")}
          <Select
            value={settings.keepDays}
            onChange={(keepDays) => onChange({ keepDays })}
            // A value set by hand in the file stays selectable.
            options={[...new Set([...KEEP_DAYS, settings.keepDays])].map((days) => ({
              value: days,
              label: days === 0 ? t("settings.logKeepForever") : t("settings.logKeepDays", { count: days }),
            }))}
          />
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

/** The app's name and version, where its data is, the privacy policy and the third-party licenses. */
