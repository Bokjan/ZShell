import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type { TransferProgress } from "../lib/api";
import { formatSize } from "../lib/format";

export interface Transfer {
  id: string;
  kind: "upload" | "download";
  label: string;
  progress: TransferProgress | null;
  status: "running" | "done" | "error" | "cancelled";
  error?: string;
  /** Local paths created by a finished download. */
  results?: string[];
}

interface Props {
  transfers: Transfer[];
  onCancel(id: string): void;
  onDismiss(id: string): void;
  onClearFinished(): void;
}

export function TransferList({ transfers, onCancel, onDismiss, onClearFinished }: Props) {
  const { t } = useTranslation();
  if (transfers.length === 0) return null;

  const detail = (transfer: Transfer) => {
    const p = transfer.progress;
    if (transfer.status === "error") return transfer.error;
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
        {transfers.some((t) => t.status !== "running") && (
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
                <span className="transfer-kind">{transfer.kind === "upload" ? "↑" : "↓"}</span>
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
                {transfer.status !== "running" && (
                  <button className="link-button" title={t("transfer.remove")} onClick={() => onDismiss(transfer.id)}>
                    ×
                  </button>
                )}
              </div>
              <div className="progress">
                <div style={{ width: `${percent}%` }} />
              </div>
              <div className="transfer-detail" title={transfer.error}>
                {detail(transfer)}
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
