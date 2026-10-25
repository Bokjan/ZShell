import { useEffect, useEffectEvent, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { dirname, join } from "@tauri-apps/api/path";

import { announce } from "../../lib/announce";
import { errorCode, errorMessage, notConnected, sftp, type FileEntry, type SessionId, type TransferEnd } from "../../ipc/api";
import { open, save } from "../../ipc/fileDialog";
import { basename } from "../../lib/format";
import { storedString, storeString } from "../../lib/storage";
import { finished, transferLabel, type Transfer } from "../TransferList";
import { useLatest } from "../../hooks/latest";
import { attempt } from "../../lib/attempt";
import { parentPath } from "../../lib/remotePath";
import type { Confirm } from "./useConfirmQueue";

const DOWNLOAD_TO_KEY = "zshell.downloadToDirectory";

interface Options {
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether the panel is in sight (questions about background saves are announced if not). */
  active: boolean;
  /** The folder shown, and its entries. */
  cwdRef: RefObject<string | null>;
  entries: FileEntry[];
  /** Lists `path` again (see `useSftpListing`). */
  load(path: string): Promise<unknown>;
  ask(question: Confirm): void;
  fail(e: unknown): void;
  /** How many uploads and downloads are running, each time that changes. */
  onTransfers(count: number): void;
  /** How many files are open in a local editor, each time that changes. */
  onEdits(count: number): void;
}

/**
 * The file panel's transfers: uploads, downloads, and files open in another application,
 * whose saves are uploaded through the current connection.
 */
export function useTransfers({ sessionId, connected, active, cwdRef, entries, load, ask, fail, onTransfers, onEdits }: Options) {
  const { t } = useTranslation();
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const transfersRef = useLatest(transfers);
  const sessionRef = useLatest({ sessionId, connected });
  const activeRef = useLatest(active);

  // Finished and failed transfers are announced to screen readers: the list may be out of
  // sight, or the panel closed.
  const statuses = useRef(new Map<string, Transfer["status"]>());
  useEffect(() => {
    for (const transfer of transfers) {
      if (statuses.current.get(transfer.id) === transfer.status) continue;
      statuses.current.set(transfer.id, transfer.status);
      if (transfer.status === "done") announce(t("announce.transferDone", { name: transferLabel(t, transfer.names) }));
      else if (transfer.status === "error")
        announce(t("announce.transferFailed", { name: transferLabel(t, transfer.names), error: transfer.error ?? "" }));
    }
  }, [transfers, t]);

  const add = (transfer: Transfer) => setTransfers((ts) => [transfer, ...ts]);
  const update = (id: string, patch: Partial<Transfer>) => setTransfers((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  // What the ones no longer listed created can't be deleted from here any more.
  const remove = (id: string) => {
    void sftp.forget([id]);
    setTransfers((ts) => ts.filter((t) => t.id !== id));
  };
  const clearFinished = () => {
    const ids = transfersRef.current.filter(finished).map((t) => t.id);
    if (ids.length > 0) void sftp.forget(ids);
    setTransfers((ts) => ts.filter((t) => !finished(t)));
  };
  const failed = (id: string, e: unknown, patch: Partial<Transfer> = {}) =>
    update(id, {
      ...patch,
      ...(errorCode(e) === "transfer.cancelled" ? { status: "cancelled" } : { status: "error", error: errorMessage(e) }),
    });

  /**
   * The session to transfer through when the transfer starts: the connection may have been
   * replaced while a question or a file dialog was open.
   */
  const currentSession = (): SessionId => {
    const { sessionId } = sessionRef.current;
    if (sessionId == null) throw notConnected(t("sftp.disconnected"));
    return sessionId;
  };

  const run = async (kind: Transfer["kind"], names: string[], work: (id: string) => Promise<TransferEnd>) => {
    const id = crypto.randomUUID();
    add({ id, kind, names, progress: null, status: "running" });
    return attempt(
      async () => {
        const end = await work(id);
        const left = { results: end.paths, created: end.created, replaced: end.replaced };
        if (end.error) failed(id, end.error, left);
        else update(id, { status: "done", ...left });
      },
      {
        onError: (e) => failed(id, e),
      },
    );
  };

  /** Uploads into `remoteDir`, asking first about replacing what is there. */
  const upload = async (localPaths: string[], remoteDir: string) => {
    if (sessionId == null || localPaths.length === 0) return;
    const start = () =>
      run("upload", localPaths.map(basename), async (id) => {
        const end = await sftp.upload(currentSession(), id, localPaths, remoteDir, (progress) => update(id, { progress }));
        if (cwdRef.current === remoteDir) void load(remoteDir);
        return end;
      });
    // Null when the folder can't be listed: whether anything would be replaced is unknown.
    const names =
      remoteDir === cwdRef.current
        ? entries.map((e) => e.name)
        : await sftp.list(sessionId, remoteDir).then(
            (listing) => listing.entries.map((e) => e.name),
            () => null,
          );
    const existing = new Set(names);
    const conflicts = localPaths.map(basename).filter((name) => existing.has(name));
    if (names && conflicts.length === 0) return void start();
    ask({
      title: t("sftp.replaceTitle"),
      message: names ? t("sftp.replaceMessage", { names: conflicts.join("\n") }) : t("sftp.replaceUnknownMessage", { path: remoteDir }),
      confirmLabel: t("sftp.replace"),
      danger: true,
      action: start,
    });
  };

  /** Downloads into `localDir`, or the download folder from the settings. */
  const download = (targets: FileEntry[], localDir: string | null = null) => {
    if (sessionId == null || targets.length === 0) return;
    void run(
      "download",
      targets.map((e) => e.name),
      async (id) =>
        sftp.download(
          currentSession(),
          id,
          targets.map((e) => e.path),
          localDir,
          (progress) => update(id, { progress }),
        ),
    );
  };

  /** A single file is saved under a name of the user's choosing; anything else into a folder. */
  const downloadTo = async (targets: FileEntry[]) => {
    if (sessionId == null || targets.length === 0) return;
    const last = storedString(DOWNLOAD_TO_KEY);
    const [first] = targets;
    if (targets.length === 1 && !first.isDir) {
      const defaultPath = last ? await join(last, first.name).catch(() => first.name) : first.name;
      const path = await save({ defaultPath, title: t("sftp.downloadToTitle") }).catch(() => null);
      if (!path) return;
      storeString(DOWNLOAD_TO_KEY, await dirname(path));
      void run("download", [first.name], async (id) =>
        sftp.downloadAs(currentSession(), id, first.path, path, (progress) => update(id, { progress })),
      );
      return;
    }
    const dir = await open({ directory: true, defaultPath: last ?? undefined, title: t("sftp.downloadToFolderTitle") }).catch(() => null);
    if (typeof dir !== "string") return;
    storeString(DOWNLOAD_TO_KEY, dir);
    download(targets, dir);
  };

  /** Asks, then permanently deletes the files and folders an ended upload or download created. */
  const deleteLeft = (id: string) => {
    const transfer = transfersRef.current.find((t) => t.id === id);
    if (!transfer?.created) return;
    const upload = transfer.kind === "upload";
    const count = transfer.created;
    const message = t(upload ? "transfer.deleteUploadedMessage" : "transfer.deleteDownloadedMessage", { count });
    const replaced = transfer.replaced ? t("transfer.deleteReplacedNote", { count: transfer.replaced }) : null;
    ask({
      title: t("transfer.deleteTitle"),
      message: replaced ? `${message}\n\n${replaced}` : message,
      confirmLabel: t("transfer.deleteConfirm"),
      danger: true,
      action: () => {
        update(id, { deletion: { state: "deleting" } });
        void attempt(
          async () => {
            const { deleted, changed } = await sftp.deleteLeft(id, upload ? currentSession() : null);
            update(id, { deletion: { state: "deleted", count: deleted, changed } });
          },
          {
            onError: (e) => {
              // What is left can be deleted again.
              update(id, { deletion: undefined });
              fail(e);
            },
          },
        );
      },
    });
  };

  // Editing in a local editor: each save is uploaded through the current connection; saves
  // made while disconnected wait for the reconnection, and saves made during an upload are
  // uploaded once it ends (by id: whether another save came in meanwhile, and whether one
  // of them was told to replace a changed file).
  const editUploads = useRef(new Map<string, { again: boolean; force: boolean }>());
  const uploadEdit = async (id: string, force = false) => {
    const { sessionId, connected } = sessionRef.current;
    const transfer = transfersRef.current.find((t) => t.id === id);
    if (!transfer || transfer.status !== "editing") return;
    if (sessionId == null || !connected) return update(id, { save: { state: "waiting" } });
    const queued = editUploads.current.get(id);
    if (queued) return void editUploads.current.set(id, { again: true, force: queued.force || force });
    editUploads.current.set(id, { again: false, force: false });
    update(id, { save: { state: "uploading" } });
    return attempt(
      async () => {
        await sftp.editUpload(sessionId, id, force);
        update(id, { save: { state: "uploaded", at: Date.now() } });
        const dir = cwdRef.current;
        if (transfer.remotePath && dir === parentPath(transfer.remotePath)) void load(dir);
      },
      {
        onError: (e) => {
          if (errorCode(e) !== "edit.conflict") {
            // Saved in another app: the transfer list may be out of sight, or the panel closed.
            announce(t("announce.editUploadFailed", { name: transfer.names[0], error: errorMessage(e) }));
            return update(id, { save: { state: "failed", error: errorMessage(e) } });
          }
          // Until the user decides. The question waits in the panel if it isn't shown.
          update(id, { save: { state: "skipped" } });
          if (!activeRef.current) announce(t("announce.editConflict", { name: transfer.names[0] }));
          ask({
            title: t("sftp.editConflictTitle"),
            message: t("sftp.editConflictMessage", { name: transfer.names[0] }),
            confirmLabel: t("sftp.editConflictReplace"),
            danger: true,
            action: () => void uploadEdit(id, true),
            key: `edit:${id}`,
          });
        },
        cleanup: () => {
          const queued = editUploads.current.get(id);
          editUploads.current.delete(id);
          if (queued?.again) void uploadEditRef.current(id, queued.force);
        },
      },
    );
  };
  const uploadEditRef = useLatest(uploadEdit);

  useEffect(() => {
    if (sessionId == null || !connected) return;
    for (const transfer of transfersRef.current) {
      if (transfer.status === "editing" && transfer.save?.state === "waiting") void uploadEditRef.current(transfer.id);
    }
  }, [sessionId, connected]);

  const running = transfers.filter((t) => t.status === "running").length;
  const reportTransfers = useEffectEvent(onTransfers);
  useEffect(() => reportTransfers(running), [running]);
  const editing = transfers.filter((t) => t.status === "editing").length;
  const reportEdits = useEffectEvent(onEdits);
  useEffect(() => reportEdits(editing), [editing]);

  // Closing the pane stops watching its files and cancels its transfers, which would otherwise
  // go on out of sight on a connection that other panes share.
  useEffect(
    () => () => {
      for (const transfer of transfersRef.current) {
        if (transfer.status === "editing") void sftp.editStop(transfer.id);
        else if (transfer.status === "running") void sftp.cancel(transfer.id);
      }
      const ended = transfersRef.current.filter(finished).map((t) => t.id);
      if (ended.length > 0) void sftp.forget(ended);
    },
    [],
  );

  const openInEditor = async (entry: FileEntry) => {
    if (sessionId == null) return;
    const current = transfersRef.current.find((t) => t.status === "editing" && t.remotePath === entry.path);
    if (current) return void sftp.editReopen(current.id).catch(fail);
    const id = crypto.randomUUID();
    add({ id, kind: "edit", names: [entry.name], remotePath: entry.path, progress: null, status: "running" });
    try {
      await sftp.editOpen(
        sessionId,
        id,
        entry.path,
        (progress) => update(id, { progress }),
        () => void uploadEditRef.current(id),
      );
      update(id, { status: "editing" });
    } catch (e) {
      failed(id, e);
    }
  };

  const stopEditing = (id: string) => {
    void sftp.editStop(id);
    remove(id);
  };

  return {
    transfers,
    add,
    update,
    remove,
    clearFinished,
    failed,
    upload,
    download,
    downloadTo,
    deleteLeft,
    openInEditor,
    stopEditing,
  };
}
