import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type { TransferProgress } from "../ipc/api";
import { useLocale } from "../hooks/locale";
import { useStableHandlers } from "../hooks/stableHandlers";
import { useStableProps } from "../hooks/stableProps";
import { IconButton } from "./IconButton";
import { ArrowIcon, CloseIcon, PencilIcon } from "./icons";

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
  { state: "uploading" | "waiting" | "skipped" } | { state: "uploaded"; at: number } | { state: "failed"; error: string };

interface Props {
  transfers: Transfer[];
  /** What the transfers' channel carries, shown at the header's end. */
  traffic?: ReactNode;
  onCancel(id: string): void;
  onDismiss(id: string): void;
  onClearFinished(): void;
  /** Opens a file being edited in the editor again. */
  onReopen(id: string): void;
  /** Stops watching a file being edited. */
  onStopEditing(id: string): void;
}

const KIND_ICONS = {
  upload: <ArrowIcon direction="up" size={12} />,
  download: <ArrowIcon direction="down" size={12} />,
  edit: <PencilIcon size={12} />,
};

const finished = (transfer: Transfer) => transfer.status !== "running" && transfer.status !== "editing";

export function TransferList({ transfers, traffic, onCancel, onDismiss, onClearFinished, onReopen, onStopEditing }: Props) {
  const { t } = useTranslation();
  // The same from render to render, so that a transfer renders again only when it changes.
  const actions = useStableHandlers<TransferActions>({ onCancel, onDismiss, onReopen, onStopEditing });
  if (transfers.length === 0) return null;

  return (
    <section className="transfers">
      <header>
        <span className="transfers-title">{t("transfer.title")}</span>
        {transfers.some(finished) && (
          <button className="link-button" onClick={onClearFinished}>
            {t("transfer.clearFinished")}
          </button>
        )}
        {/* Last, so that the button coming and going doesn't move it. */}
        {traffic}
      </header>
      <ul>
        {transfers.map((transfer) => (
          <TransferItem key={transfer.id} transfer={transfer} actions={actions} />
        ))}
      </ul>
    </section>
  );
}

/** What a transfer's buttons do: one object for every transfer, the same from render to render. */
type TransferActions = Pick<Props, "onCancel" | "onDismiss" | "onReopen" | "onStopEditing">;

/**
 * One transfer, its own component so that React Compiler memoizes it: the list renders again
 * with each running transfer's progress, ten times a second, and the others keep their
 * elements and handlers.
 */
function TransferItem(props: { transfer: Transfer; actions: TransferActions }) {
  useStableProps("TransferItem", props, ["actions"]);
  const { transfer, actions } = props;
  const { t, i18n } = useTranslation();
  const { formatSize, widestSizes } = useLocale();
  const { id } = transfer;
  const p = transfer.progress;
  const percent = p && p.total > 0 ? (p.transferred / p.total) * 100 : transfer.status === "done" ? 100 : 0;

  const detail = () => {
    if (transfer.status === "error") return transfer.error;
    if (transfer.status === "editing") {
      const save = transfer.save;
      if (!save) return t("transfer.editing");
      if (save.state === "uploaded") return t("transfer.editUploaded", { time: new Date(save.at).toLocaleTimeString(i18n.language) });
      if (save.state === "failed") return t("transfer.editFailed", { message: save.error });
      return t(`transfer.edit.${save.state}`);
    }
    if (transfer.status === "cancelled") return t("transfer.cancelled");
    return t("transfer.preparing");
  };

  // The file being copied on the left; the amounts on the right, each as wide as it will
  // ever be, so that neither moves as the numbers grow.
  const progress = (p: TransferProgress) => {
    const size = (transferred: string) => t("transfer.size", { transferred, total: formatSize(p.total) });
    const files = (done: number) => t("transfer.files", { done, count: p.filesTotal });
    return (
      <div className="transfer-detail transfer-progress">
        <span className="transfer-current" title={transfer.status === "running" ? p.current : undefined}>
          {transfer.status === "running" ? p.current : ""}
        </span>
        <Steady text={size(formatSize(p.transferred))} samples={widestSizes(p.total).map(size)} />
        <Steady text={files(p.filesDone)} samples={[files(p.filesTotal)]} />
      </div>
    );
  };

  return (
    <li className={transfer.status}>
      <div className="transfer-row">
        <span className="transfer-kind" role="img" aria-label={t(`transfer.kind.${transfer.kind}`)}>
          {KIND_ICONS[transfer.kind]}
        </span>
        <span className="transfer-label" title={transfer.label}>
          {transfer.label}
        </span>
        {transfer.status === "running" && (
          <button className="link-button" onClick={() => actions.onCancel(id)}>
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
            <button className="link-button" onClick={() => actions.onReopen(id)}>
              {t("transfer.reopen")}
            </button>
            <button className="link-button" onClick={() => actions.onStopEditing(id)}>
              {t("transfer.stopEditing")}
            </button>
          </>
        )}
        {finished(transfer) && (
          <IconButton className="link-button" label={t("transfer.remove")} onClick={() => actions.onDismiss(id)}>
            <CloseIcon size={12} />
          </IconButton>
        )}
      </div>
      {transfer.status !== "editing" && (
        <div className="progress">
          <div style={{ width: `${percent}%` }} />
        </div>
      )}
      {p && (transfer.status === "running" || transfer.status === "done") ? (
        progress(p)
      ) : (
        <div className="transfer-detail" title={transfer.save?.state === "failed" ? transfer.save.error : transfer.error}>
          {detail()}
        </div>
      )}
    </li>
  );
}

/** `text`, as wide as the widest of `samples` (laid under it, unseen), so that it keeps its width as it changes. */
function Steady({ text, samples }: { text: string; samples: string[] }) {
  return (
    <span className="steady">
      {samples.map((sample, i) => (
        <span key={i} aria-hidden="true">
          {sample}
        </span>
      ))}
      <span>{text}</span>
    </span>
  );
}
