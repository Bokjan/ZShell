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
  if (transfers.length === 0) return null;
  return (
    <section className="transfers">
      <header>
        <span>Transfers</span>
        {transfers.some((t) => t.status !== "running") && (
          <button className="link-button" onClick={onClearFinished}>
            Clear finished
          </button>
        )}
      </header>
      <ul>
        {transfers.map((t) => {
          const p = t.progress;
          const percent = p && p.total > 0 ? (p.transferred / p.total) * 100 : t.status === "done" ? 100 : 0;
          return (
            <li key={t.id} className={t.status}>
              <div className="transfer-row">
                <span className="transfer-kind">{t.kind === "upload" ? "↑" : "↓"}</span>
                <span className="transfer-label" title={t.label}>
                  {t.label}
                </span>
                {t.status === "running" && (
                  <button className="link-button" onClick={() => onCancel(t.id)}>
                    Cancel
                  </button>
                )}
                {t.status === "done" && t.results?.[0] && (
                  <button className="link-button" onClick={() => void revealItemInDir(t.results![0])}>
                    Show
                  </button>
                )}
                {t.status !== "running" && (
                  <button className="link-button" title="Remove" onClick={() => onDismiss(t.id)}>
                    ×
                  </button>
                )}
              </div>
              <div className="progress">
                <div style={{ width: `${percent}%` }} />
              </div>
              <div className="transfer-detail" title={t.error}>
                {t.status === "error"
                  ? t.error
                  : t.status === "cancelled"
                    ? "Cancelled"
                    : p
                      ? `${formatSize(p.transferred)} / ${formatSize(p.total)} · ${p.filesDone}/${p.filesTotal} files${t.status === "running" && p.current ? ` · ${p.current}` : ""}`
                      : "Preparing…"}
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
