import type { ReactNode } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import type { TransferProgress } from "../ipc/api";
import { useLocale } from "../hooks/locale";
import { useStableHandlers } from "../hooks/stableHandlers";
import { useStableProps } from "../hooks/stableProps";
import { IconButton } from "./IconButton";
import { ArrowIcon, CloseIcon, PencilIcon, TrashIcon } from "./icons";

export interface Transfer {
  id: string;
  kind: "upload" | "download" | "edit";
  /** The names of what it copies, which name it in the current language (see `transferLabel`). */
  names: string[];
  progress: TransferProgress | null;
  /** "editing" once a file opened in an editor is downloaded and watched. */
  status: "running" | "editing" | "done" | "error" | "cancelled";
  error?: string;
  /** The local items an ended download left, to show. */
  results?: string[];
  /** For an ended upload or download: how many files it created, which can be deleted, and how many it wrote over. */
  created?: number;
  replaced?: number;
  /** Deleting what it created: under way, or done, with how many files were deleted and how many kept as they changed since. */
  deletion?: { state: "deleting" } | { state: "deleted"; count: number; changed: number };
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
  /** Whether the session is connected, which deleting what an upload created needs. */
  connected: boolean;
  onCancel(id: string): void;
  /** Asks, then deletes the files an ended transfer created. */
  onDelete(id: string): void;
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

/** What a transfer of `names` is called: the first name, and how many more. */
export const transferLabel = (t: TFunction, names: string[]) =>
  names.length === 1 ? names[0] : t("transfer.labelMore", { name: names[0], count: names.length - 1 });

export const finished = (transfer: Transfer) => transfer.status !== "running" && transfer.status !== "editing";

export function TransferList({
  transfers,
  traffic,
  connected,
  onCancel,
  onDelete,
  onDismiss,
  onClearFinished,
  onReopen,
  onStopEditing,
}: Props) {
  const { t } = useTranslation();
  // The same from render to render, so that a transfer renders again only when it changes.
  const actions = useStableHandlers<TransferActions>({ onCancel, onDelete, onDismiss, onReopen, onStopEditing });
  if (transfers.length === 0) return null;

  return (
    <section className="transfers">
      <header>
        <span className="transfers-title">{t("transfer.title")}</span>
        {transfers.some(finished) && (
          <button className="link" onClick={onClearFinished}>
            {t("transfer.clearFinished")}
          </button>
        )}
        {/* Last, so that the button coming and going doesn't move it. */}
        {traffic}
      </header>
      <ul>
        {transfers.map((transfer) => (
          <TransferItem key={transfer.id} transfer={transfer} connected={connected} actions={actions} />
        ))}
      </ul>
    </section>
  );
}

/** What a transfer's buttons do: one object for every transfer, the same from render to render. */
type TransferActions = Pick<Props, "onCancel" | "onDelete" | "onDismiss" | "onReopen" | "onStopEditing">;

/**
 * One transfer, its own component so that React Compiler memoizes it: the list renders again
 * with each running transfer's progress, ten times a second, and the others keep their
 * elements and handlers.
 */
function TransferItem(props: { transfer: Transfer; connected: boolean; actions: TransferActions }) {
  useStableProps("TransferItem", props, ["actions"]);
  const { transfer, connected, actions } = props;
  const { t, i18n } = useTranslation();
  const { formatSize, widestSizes } = useLocale();
  const { id } = transfer;
  const p = transfer.progress;
  const percent = p && p.total > 0 ? (p.transferred / p.total) * 100 : transfer.status === "done" ? 100 : 0;

  const deleted = transfer.deletion?.state === "deleted" ? transfer.deletion.count : null;
  // What it created can be deleted once it has ended, the files of an upload only through the connection.
  const deletable = finished(transfer) && (transfer.created ?? 0) > 0 && deleted == null;
  const upload = transfer.kind === "upload";
  const label = transferLabel(t, transfer.names);

  const detail = () => {
    if (deleted != null) {
      if (transfer.status === "cancelled") return t("transfer.cancelledDeleted", { count: deleted });
      if (transfer.status === "error") return t("transfer.failedDeleted", { error: transfer.error, count: deleted });
      return t("transfer.deleted", { count: deleted });
    }
    if (transfer.status === "error") return transfer.error;
    if (transfer.status === "editing") {
      const save = transfer.save;
      if (!save) return t("transfer.editing");
      if (save.state === "uploaded") return t("transfer.editUploaded", { time: new Date(save.at).toLocaleTimeString(i18n.language) });
      if (save.state === "failed") return t("transfer.editFailed", { message: save.error });
      return t(`transfer.edit.${save.state}`);
    }
    if (transfer.status === "cancelled") {
      return p ? t("transfer.cancelledProgress", { done: p.filesDone, count: p.filesTotal }) : t("transfer.cancelled");
    }
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
        <span className="transfer-label" title={label}>
          {label}
        </span>
        {transfer.status === "running" && (
          <button className="link" onClick={() => actions.onCancel(id)}>
            {t("transfer.cancel")}
          </button>
        )}
        {finished(transfer) && transfer.results?.[0] && deleted == null && (
          <button className="link" onClick={() => void revealItemInDir(transfer.results![0])}>
            {t("transfer.show")}
          </button>
        )}
        {deletable && (
          <IconButton
            className="link"
            label={
              upload && !connected
                ? t("transfer.deleteNeedsConnection")
                : t(upload ? "transfer.deleteUploaded" : "transfer.deleteDownloaded")
            }
            disabled={(upload && !connected) || transfer.deletion?.state === "deleting"}
            onClick={() => actions.onDelete(id)}
          >
            <TrashIcon size={12} />
          </IconButton>
        )}
        {transfer.status === "editing" && (
          <>
            <button className="link" onClick={() => actions.onReopen(id)}>
              {t("transfer.reopen")}
            </button>
            <button className="link" onClick={() => actions.onStopEditing(id)}>
              {t("transfer.stopEditing")}
            </button>
          </>
        )}
        {finished(transfer) && (
          <IconButton className="link" label={t("transfer.remove")} onClick={() => actions.onDismiss(id)}>
            <CloseIcon size={12} />
          </IconButton>
        )}
      </div>
      {transfer.status !== "editing" && (
        <div className="progress">
          <div style={{ width: `${percent}%` }} />
        </div>
      )}
      {p && (transfer.status === "running" || transfer.status === "done") && deleted == null ? (
        progress(p)
      ) : (
        <div className="transfer-detail" title={transfer.save?.state === "failed" ? transfer.save.error : transfer.error}>
          {detail()}
        </div>
      )}
      {transfer.deletion?.state === "deleted" && transfer.deletion.changed > 0 && (
        <div className="transfer-detail">{t("transfer.keptChanged", { count: transfer.deletion.changed })}</div>
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
