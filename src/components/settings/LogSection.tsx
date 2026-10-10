import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { logs } from "../../lib/api";
import { useConfirmButton } from "../../lib/confirm";
import { formatSize } from "../../lib/format";
import { isComposing } from "../../lib/platform";
import type { LogFormat, LogSettings } from "../../lib/settings";
import { FolderField, Section, Setting } from "./common";

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
  const [directory, setDirectory] = useState("");
  const [summary, setSummary] = useState<{ count: number; bytes: number } | null>(null);
  const deleteButton = useConfirmButton();
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
    if (!deleteButton.armed) {
      deleteButton.setArmed(true);
      return;
    }
    deleteButton.setArmed(false);
    logs.delete().then(refresh, console.error);
  };

  // The file name is saved when the field is left (or the dialog closes, which may not blur
  // it first), so that clearing it to type another doesn't bring the default back in the
  // middle.
  const commitFileName = () => {
    if (fileName.trim() !== settings.fileName) onChange({ fileName: fileName.trim() });
  };
  const commitLatest = useRef(commitFileName);
  commitLatest.current = commitFileName;
  useEffect(() => () => commitLatest.current(), []);

  return (
    <Section id="logs">
      <Setting>
        <FolderField
          label={t("settings.logDirectory")}
          path={directory}
          onChoose={() => void choose()}
          // Created first: it may not exist until a log is written.
          onShow={() => logs.directory(true).then(revealItemInDir).catch(console.error)}
          reset={
            settings.directory ? { label: t("settings.logDefaultDirectory"), onReset: () => onChange({ directory: "" }) } : null
          }
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
          ref={deleteButton.ref}
          onClick={deleteAll}
          onBlur={deleteButton.onBlur}
        >
          {deleteButton.armed ? t("settings.logDeleteAllConfirm", { count: summary?.count ?? 0 }) : t("settings.logDeleteAll")}
        </button>
      </div>
    </Section>
  );
}

/** The app's name and version, where its data is, the privacy policy and the third-party licenses. */
