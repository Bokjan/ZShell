import { useEffect, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { dirname, join } from "@tauri-apps/api/path";
import { open, save } from "@tauri-apps/plugin-dialog";

import { announce } from "../../lib/announce";
import { errorCode, errorMessage, sftp, type FileEntry, type SessionId } from "../../lib/api";
import { basename } from "../../lib/format";
import { storedString, storeString } from "../../lib/storage";
import type { Transfer } from "../TransferList";
import type { Confirm } from "./useConfirmQueue";

const DOWNLOAD_TO_KEY = "zshell.downloadToDirectory";

export const parentPath = (path: string) => path.replace(/\/[^/]+\/?$/, "") || "/";

interface Options {
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether the panel is in sight (questions about background saves are announced if not). */
  active: boolean;
  /** The folder shown, and its entries. */
  cwdRef: RefObject<string | null>;
  entries: FileEntry[];
  /** Lists `path` again (see `useSftpListing`). */
  load(path: string): Promise<void>;
  ask(question: Confirm): void;
  fail(e: unknown): void;
  /** How many uploads and downloads are running, each time that changes. */
  onTransfers(count: number): void;
}

/**
 * The file panel's transfers: uploads, downloads, and files open in another application,
 * whose saves are uploaded through the current connection.
 */
export function useTransfers({ sessionId, connected, active, cwdRef, entries, load, ask, fail, onTransfers }: Options) {
  const { t } = useTranslation();
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const transfersRef = useRef(transfers);
  transfersRef.current = transfers;
  const sessionRef = useRef({ sessionId, connected });
  sessionRef.current = { sessionId, connected };
  const activeRef = useRef(active);
  activeRef.current = active;

  // Finished and failed transfers are announced to screen readers: the list may be out of
  // sight, or the panel closed.
  const statuses = useRef(new Map<string, Transfer["status"]>());
  useEffect(() => {
    for (const transfer of transfers) {
      if (statuses.current.get(transfer.id) === transfer.status) continue;
      statuses.current.set(transfer.id, transfer.status);
      if (transfer.status === "done") announce(t("announce.transferDone", { name: transfer.label }));
      else if (transfer.status === "error") announce(t("announce.transferFailed", { name: transfer.label, error: transfer.error ?? "" }));
    }
  }, [transfers, t]);

  const add = (transfer: Transfer) => setTransfers((ts) => [transfer, ...ts]);
  const update = (id: string, patch: Partial<Transfer>) => setTransfers((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  const remove = (id: string) => setTransfers((ts) => ts.filter((t) => t.id !== id));
  const clearFinished = () => setTransfers((ts) => ts.filter((t) => t.status === "running" || t.status === "editing"));
  const failed = (id: string, e: unknown) =>
    update(id, errorCode(e) === "transfer.cancelled" ? { status: "cancelled" } : { status: "error", error: errorMessage(e) });

  const labelFor = (names: string[]) => (names.length === 1 ? names[0] : t("transfer.labelMore", { name: names[0], count: names.length - 1 }));

  const run = async (kind: Transfer["kind"], names: string[], work: (id: string) => Promise<string[] | void>) => {
    const id = crypto.randomUUID();
    add({ id, kind, label: labelFor(names), progress: null, status: "running" });
    try {
      const results = await work(id);
      update(id, { status: "done", results: results ?? undefined });
    } catch (e) {
      failed(id, e);
    }
  };

  /** Uploads into `remoteDir`, asking first about replacing what is there. */
  const upload = async (localPaths: string[], remoteDir: string) => {
    if (sessionId == null || localPaths.length === 0) return;
    const start = () =>
      run("upload", localPaths.map(basename), async (id) => {
        await sftp.upload(sessionId, id, localPaths, remoteDir, (progress) => update(id, { progress }));
        if (cwdRef.current === remoteDir) void load(remoteDir);
      });
    const names =
      remoteDir === cwdRef.current
        ? entries.map((e) => e.name)
        : await sftp.list(sessionId, remoteDir).then((listing) => listing.entries.map((e) => e.name), () => []);
    const existing = new Set(names);
    const conflicts = localPaths.map(basename).filter((name) => existing.has(name));
    if (conflicts.length === 0) return void start();
    ask({
      title: t("sftp.replaceTitle"),
      message: t("sftp.replaceMessage", { names: conflicts.join("\n") }),
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
      (id) => sftp.download(sessionId, id, targets.map((e) => e.path), localDir, (progress) => update(id, { progress })),
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
      void run("download", [first.name], async (id) => [
        await sftp.downloadAs(sessionId, id, first.path, path, (progress) => update(id, { progress })),
      ]);
      return;
    }
    const dir = await open({ directory: true, defaultPath: last ?? undefined, title: t("sftp.downloadToFolderTitle") }).catch(() => null);
    if (typeof dir !== "string") return;
    storeString(DOWNLOAD_TO_KEY, dir);
    download(targets, dir);
  };

  // Editing in a local editor: each save is uploaded through the current connection; saves
  // made while disconnected wait for the reconnection, and saves made during an upload are
  // uploaded once it ends (by id: whether another save came in meanwhile).
  const editUploads = useRef(new Map<string, boolean>());
  const uploadEdit = async (id: string, force = false) => {
    const { sessionId, connected } = sessionRef.current;
    const transfer = transfersRef.current.find((t) => t.id === id);
    if (!transfer || transfer.status !== "editing") return;
    if (sessionId == null || !connected) return update(id, { save: { state: "waiting" } });
    if (editUploads.current.has(id)) return void editUploads.current.set(id, true);
    editUploads.current.set(id, false);
    update(id, { save: { state: "uploading" } });
    try {
      await sftp.editUpload(sessionId, id, force);
      update(id, { save: { state: "uploaded", at: Date.now() } });
      const dir = cwdRef.current;
      if (transfer.remotePath && dir === parentPath(transfer.remotePath)) void load(dir);
    } catch (e) {
      if (errorCode(e) !== "edit.conflict") {
        // Saved in another app: the transfer list may be out of sight, or the panel closed.
        announce(t("announce.editUploadFailed", { name: transfer.label, error: errorMessage(e) }));
        return update(id, { save: { state: "failed", error: errorMessage(e) } });
      }
      // Until the user decides. The question waits in the panel if it isn't shown.
      update(id, { save: { state: "skipped" } });
      if (!activeRef.current) announce(t("announce.editConflict", { name: transfer.label }));
      ask({
        title: t("sftp.editConflictTitle"),
        message: t("sftp.editConflictMessage", { name: transfer.label }),
        confirmLabel: t("sftp.editConflictReplace"),
        danger: true,
        action: () => void uploadEdit(id, true),
        key: `edit:${id}`,
      });
    } finally {
      const again = editUploads.current.get(id);
      editUploads.current.delete(id);
      if (again) void uploadEditRef.current(id);
    }
  };
  const uploadEditRef = useRef(uploadEdit);
  uploadEditRef.current = uploadEdit;

  useEffect(() => {
    if (sessionId == null || !connected) return;
    for (const transfer of transfersRef.current) {
      if (transfer.status === "editing" && transfer.save?.state === "waiting") void uploadEditRef.current(transfer.id);
    }
  }, [sessionId, connected]);

  const running = transfers.filter((t) => t.status === "running").length;
  const onTransfersRef = useRef(onTransfers);
  onTransfersRef.current = onTransfers;
  useEffect(() => onTransfersRef.current(running), [running]);

  // Closing the pane stops watching its files and cancels its transfers, which would otherwise
  // go on out of sight on a connection that other panes share.
  useEffect(
    () => () => {
      for (const transfer of transfersRef.current) {
        if (transfer.status === "editing") void sftp.editStop(transfer.id);
        else if (transfer.status === "running") void sftp.cancel(transfer.id);
      }
    },
    [],
  );

  const openInEditor = async (entry: FileEntry) => {
    if (sessionId == null) return;
    const current = transfersRef.current.find((t) => t.status === "editing" && t.remotePath === entry.path);
    if (current) return void sftp.editReopen(current.id).catch(fail);
    const id = crypto.randomUUID();
    add({ id, kind: "edit", label: entry.name, remotePath: entry.path, progress: null, status: "running" });
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

  return { transfers, add, update, remove, clearFinished, failed, labelFor, upload, download, downloadTo, openInEditor, stopEditing };
}
