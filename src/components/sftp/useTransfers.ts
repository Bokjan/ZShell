import { useEffect, useEffectEvent, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { dirname, join } from "@tauri-apps/api/path";

import { announce } from "../../lib/announce";
import { errorCode, errorMessage, notConnected, sftp, type FileEntry, type SessionId, type TransferEnd } from "../../ipc/api";
import { open, save } from "../../ipc/fileDialog";
import { basename } from "../../lib/format";
import { storedString, storeString } from "../../lib/storage";
import { finished, NO_DELETION, transferLabel, type Deletion, type Transfer } from "../TransferList";
import { useLatest } from "../../hooks/latest";
import { attempt } from "../../lib/attempt";
import { parentPath } from "../../lib/remotePath";
import { TransferProgresses } from "../../lib/transferProgress";
import type { Confirm } from "./useConfirmQueue";

const DOWNLOAD_TO_KEY = "zshell.downloadToDirectory";

interface Options {
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether the pane is connecting, or waiting to reconnect, after its connection was lost. */
  reconnecting: boolean;
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
export function useTransfers({
  sessionId,
  connected,
  reconnecting,
  active,
  cwdRef,
  entries,
  load,
  ask,
  fail,
  onTransfers,
  onEdits,
}: Options) {
  const { t } = useTranslation();
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  // Apart from the transfers, so that each report renders only the transfer's row.
  const [progress] = useState(() => new TransferProgresses());
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
      else if (transfer.status === "waiting") announce(t("announce.transferWaiting", { name: transferLabel(t, transfer.names) }));
      else if (transfer.status === "error")
        announce(t("announce.transferFailed", { name: transferLabel(t, transfer.names), error: transfer.error ?? "" }));
    }
  }, [transfers, t]);

  const add = (transfer: Transfer) => setTransfers((ts) => [transfer, ...ts]);
  const update = (id: string, patch: Partial<Transfer>) => setTransfers((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  /** Changes transfer `id` from what deleting its files has done so far. */
  const updateDeletion = (id: string, change: (deletion: Deletion) => Partial<Transfer>) =>
    setTransfers((ts) => ts.map((t) => (t.id === id ? { ...t, ...change(t.deletion ?? NO_DELETION) } : t)));
  // What the ones no longer listed created can't be deleted from here any more, nor can those
  // that failed be gone on with.
  const remove = (id: string) => {
    void sftp.forget([id], sessionRef.current.sessionId);
    progress.forget([id]);
    setTransfers((ts) => ts.filter((t) => t.id !== id));
  };
  const clearFinished = () => {
    const ids = transfersRef.current.filter(finished).map((t) => t.id);
    if (ids.length > 0) void sftp.forget(ids, sessionRef.current.sessionId);
    progress.forget(ids);
    setTransfers((ts) => ts.filter((t) => !finished(t)));
  };
  const failed = (id: string, e: unknown, patch: Partial<Transfer> = {}) =>
    update(id, {
      ...patch,
      ...(errorCode(e) === "transfer.cancelled" ? { status: "cancelled" } : { status: "error", error: errorMessage(e) }),
    });
  /**
   * Transfer `id` ended through `session`: done, or failed, when it may go on after the tab
   * reconnects (its connection was lost) or when retried.
   */
  const settle = (id: string, end: TransferEnd, session: SessionId) => {
    const left = { results: end.paths, created: end.created, folders: end.folders, replaced: end.replaced };
    if (!end.error) update(id, { status: "done", ...left });
    else if (end.resume === "lost") update(id, { status: "waiting", error: errorMessage(end.error), lostOn: session, ...left });
    else failed(id, end.error, { ...left, retry: end.resume === "retry" });
  };

  /**
   * The session to transfer through when the transfer starts: the connection may have been
   * replaced while a question or a file dialog was open.
   */
  const currentSession = (): SessionId => {
    const { sessionId } = sessionRef.current;
    if (sessionId == null) throw notConnected(t("sftp.disconnected"));
    return sessionId;
  };

  /** Starts a transfer, which `work` runs through the current session. */
  const run = async (
    kind: Transfer["kind"],
    names: string[],
    work: (id: string, session: SessionId) => Promise<TransferEnd>,
    patch: Partial<Transfer> = {},
  ) => {
    const id = crypto.randomUUID();
    add({ id, kind, names, status: "running", ...patch });
    return attempt(
      async () => {
        const session = currentSession();
        settle(id, await work(id, session), session);
      },
      {
        onError: (e) => failed(id, e),
      },
    );
  };

  /**
   * Goes on with a failed transfer through the current session: after a lost connection, or
   * retried (`check`). Once at a time: an effect may ask again before it shows as running.
   */
  const resuming = useRef(new Set<string>());
  const resume = (id: string, check: boolean) => {
    const { sessionId, connected } = sessionRef.current;
    const transfer = transfersRef.current.find((t) => t.id === id);
    if (sessionId == null || !connected || !transfer || resuming.current.has(id)) return;
    resuming.current.add(id);
    update(id, { status: "running", error: undefined, retry: undefined, lostOn: undefined });
    void attempt(
      async () => {
        const end = await sftp.resume(sessionId, id, check, (p) => progress.set(id, p));
        if (transfer.remoteDir && cwdRef.current === transfer.remoteDir) void load(transfer.remoteDir);
        settle(id, end, sessionId);
      },
      {
        onError: (e) => failed(id, e),
        cleanup: () => resuming.current.delete(id),
      },
    );
  };
  const resumeRef = useLatest(resume);

  // Those cut short by a lost connection go on once the tab is connected again (a new
  // session); if it doesn't reconnect by itself, they can be retried.
  useEffect(() => {
    if (sessionId == null || !connected) return;
    for (const transfer of transfers) {
      if (transfer.status === "waiting" && transfer.lostOn !== sessionId) resumeRef.current(transfer.id, false);
    }
  }, [sessionId, connected, transfers]);
  useEffect(() => {
    if (connected || reconnecting || !transfers.some((t) => t.status === "waiting")) return;
    setTransfers((ts) => ts.map((t) => (t.status === "waiting" ? { ...t, status: "error", retry: true } : t)));
  }, [connected, reconnecting, transfers]);

  /** Cancels a running transfer, or gives up on going on with one cut short by a lost connection. */
  const cancel = (id: string) => {
    const transfer = transfersRef.current.find((t) => t.id === id);
    if (transfer?.status === "running") return void sftp.cancel(id);
    if (transfer?.status !== "waiting") return;
    void sftp.discard(id, sessionRef.current.sessionId);
    update(id, { status: "cancelled", lostOn: undefined });
  };

  /** Uploads into `remoteDir`, asking first about replacing what is there. */
  const upload = async (localPaths: string[], remoteDir: string) => {
    if (sessionId == null || localPaths.length === 0) return;
    const start = () =>
      run(
        "upload",
        localPaths.map(basename),
        async (id, session) => {
          const end = await sftp.upload(session, id, localPaths, remoteDir, (p) => progress.set(id, p));
          if (cwdRef.current === remoteDir) void load(remoteDir);
          return end;
        },
        { remoteDir },
      );
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
      async (id, session) =>
        sftp.download(
          session,
          id,
          targets.map((e) => e.path),
          localDir,
          (p) => progress.set(id, p),
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
      void run("download", [first.name], async (id, session) => sftp.downloadAs(session, id, first.path, path, (p) => progress.set(id, p)));
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
    if (!transfer || !(transfer.created || transfer.folders)) return;
    const upload = transfer.kind === "upload";
    const message = transfer.created
      ? t(upload ? "transfer.deleteUploadedMessage" : "transfer.deleteDownloadedMessage", { count: transfer.created })
      : t(upload ? "transfer.deleteUploadedFoldersMessage" : "transfer.deleteDownloadedFoldersMessage", { count: transfer.folders });
    const replaced = transfer.replaced ? t("transfer.deleteReplacedNote", { count: transfer.replaced }) : null;
    ask({
      title: t("transfer.deleteTitle"),
      message: replaced ? `${message}\n\n${replaced}` : message,
      confirmLabel: t("transfer.deleteConfirm"),
      danger: true,
      action: () => {
        updateDeletion(id, (deletion) => ({ deletion: { ...deletion, deleting: true } }));
        void attempt(
          async () => {
            const done = await sftp.deleteLeft(id, upload ? currentSession() : null);
            // Added to what earlier tries deleted; what is still to delete can be deleted again.
            updateDeletion(id, (deletion) => ({
              created: done.created,
              folders: done.folders,
              deletion: {
                deleting: false,
                files: deletion.files + done.deleted,
                folders: deletion.folders + done.deletedFolders,
                changed: deletion.changed + done.changed,
              },
            }));
            if (done.error) fail(done.error);
          },
          {
            onError: (e) => {
              updateDeletion(id, (deletion) => ({ deletion: { ...deletion, deleting: false } }));
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

  // Those waiting to go on count too: closing the pane gives up on them.
  const running = transfers.filter((t) => t.status === "running" || t.status === "waiting").length;
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
      const ended = transfersRef.current.filter((t) => finished(t) || t.status === "waiting").map((t) => t.id);
      if (ended.length > 0) void sftp.forget(ended, sessionRef.current.sessionId);
    },
    [],
  );

  const openInEditor = async (entry: FileEntry) => {
    if (sessionId == null) return;
    const current = transfersRef.current.find((t) => t.status === "editing" && t.remotePath === entry.path);
    if (current) return void sftp.editReopen(current.id).catch(fail);
    const id = crypto.randomUUID();
    add({ id, kind: "edit", names: [entry.name], remotePath: entry.path, status: "running" });
    try {
      await sftp.editOpen(
        sessionId,
        id,
        entry.path,
        (p) => progress.set(id, p),
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
    progress,
    add,
    update,
    remove,
    clearFinished,
    failed,
    cancel,
    /** Retries a failed transfer, copying again what is no longer as it was left. */
    retry: (id: string) => resume(id, true),
    upload,
    download,
    downloadTo,
    deleteLeft,
    openInEditor,
    stopEditing,
  };
}
