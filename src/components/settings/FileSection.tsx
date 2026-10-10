import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { sftp } from "../../lib/api";
import { basename } from "../../lib/format";
import { isMac } from "../../lib/platform";
import type { FileSettings } from "../../lib/settings";
import { FolderField, Section, Setting } from "./common";

/** Remote files: where downloads go, and the editor remote files are edited with. */
export function FileSection({ settings, onChange }: { settings: FileSettings; onChange(patch: Partial<FileSettings>): void }) {
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
        <FolderField
          label={t("settings.downloadDirectory")}
          path={directory}
          onChoose={() => void chooseDirectory()}
          reset={
            settings.downloadDirectory
              ? { label: t("settings.downloadDirectoryDefault"), onReset: () => onChange({ downloadDirectory: "" }) }
              : null
          }
        />
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
              aria-label={t("settings.editor")}
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
