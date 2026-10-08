import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type { TransferProgress } from "../lib/api";
import { formatSize } from "../lib/format";
import i18n from "../i18n";

export interface Transfer {
  id: string;
  kind: "upload" | "download" | "edit";
  label: string;
  progress: TransferProgress | null;
  /** "editing" once a file opened in an editor is downloaded and watched. */
  status: "running" | "editing" | "done" | "error" | "cancelled";
  error?: string;
  /** Local paths created by a finished download. */
  results?: string[];
  /** For an edit: the remote file. */
  remotePath?: string;
  /** For an edit: what became of the last save. */
  save?: EditSave;
}

export type EditSave =
  | { state: "uploading" | "waiting" | "skipped" }
  | { state: "uploaded"; at: number }
  | { state: "failed"; error: string };

interface Props {
  transfers: Transfer[];
  onCancel(id: string): void;
  onDismiss(id: string): void;
  onClearFinished(): void;
  /** Opens a file being edited in the editor again. */
  onReopen(id: string): void;
  /** Stops watching a file being edited. */
  onStopEditing(id: string): void;
}

const KIND_ICONS = { upload: "↑", download: "↓", edit: "✎" };

const finished = (transfer: Transfer) => transfer.status !== "running" && transfer.status !== "editing";

export function TransferList({ transfers, onCancel, onDismiss, onClearFinished, onReopen, onStopEditing }: Props) {
  const { t } = useTranslation();
  if (transfers.length === 0) return null;

  const detail = (transfer: Transfer) => {
    const p = transfer.progress;
    if (transfer.status === "error") return transfer.error;
    if (transfer.status === "editing") {
      const save = transfer.save;
      if (!save) return t("transfer.editing");
      if (save.state === "uploaded")
        return t("transfer.editUploaded", { time: new Date(save.at).toLocaleTimeString(i18n.language) });
      if (save.state === "failed") return t("transfer.editFailed", { message: save.error });
      return t(`transfer.edit.${save.state}`);
    }
    if (transfer.status === "cancelled") return t("transfer.cancelled");
    if (!p) return t("transfer.preparing");
    const progress = t("transfer.progress", {
      transferred: formatSize(p.transferred),
      total: formatSize(p.total),
      done: p.filesDone,
      count: p.filesTotal,
    });
    return transfer.status === "running" && p.current ? t("transfer.progressCurrent", { progress, file: p.current }) : progress;
  };

  return (
    <section className="transfers">
      <header>
        <span>{t("transfer.title")}</span>
        {transfers.some(finished) && (
          <button className="link-button" onClick={onClearFinished}>
            {t("transfer.clearFinished")}
          </button>
        )}
      </header>
      <ul>
        {transfers.map((transfer) => {
          const p = transfer.progress;
          const percent = p && p.total > 0 ? (p.transferred / p.total) * 100 : transfer.status === "done" ? 100 : 0;
          return (
            <li key={transfer.id} className={transfer.status}>
              <div className="transfer-row">
                <span className="transfer-kind">{KIND_ICONS[transfer.kind]}</span>
                <span className="transfer-label" title={transfer.label}>
                  {transfer.label}
                </span>
                {transfer.status === "running" && (
                  <button className="link-button" onClick={() => onCancel(transfer.id)}>
                    {t("transfer.cancel")}
                  </button>
                )}
                {transfer.status === "done" && transfer.results?.[0] && (
                  <button className="link-button" onClick={() => void revealItemInDir(transfer.results![0])}>
                    {t("transfer.show")}
                  </button>
                )}
                {transfer.status === "editing" && (
                  <>
                    <button className="link-button" onClick={() => onReopen(transfer.id)}>
                      {t("transfer.reopen")}
                    </button>
                    <button className="link-button" onClick={() => onStopEditing(transfer.id)}>
                      {t("transfer.stopEditing")}
                    </button>
                  </>
                )}
                {finished(transfer) && (
                  <button className="link-button" title={t("transfer.remove")} onClick={() => onDismiss(transfer.id)}>
                    ×
                  </button>
                )}
              </div>
              {transfer.status !== "editing" && (
                <div className="progress">
                  <div style={{ width: `${percent}%` }} />
                </div>
              )}
              <div className="transfer-detail" title={transfer.save?.state === "failed" ? transfer.save.error : transfer.error}>
                {detail(transfer)}
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
