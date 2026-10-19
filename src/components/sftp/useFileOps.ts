import { useState, type FormEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { notConnected, sftp, type FileEntry, type SessionId } from "../../ipc/api";
import { basename } from "../../lib/format";
import { joinPath } from "../../lib/remotePath";
import { useLatest } from "../../hooks/latest";
import type { Confirm } from "./useConfirmQueue";

interface Options {
  sessionId: SessionId | null;
  /** The folder shown. */
  cwd: string | null;
  /** The list, which gets the focus back after a name is entered. */
  listRef: RefObject<HTMLDivElement | null>;
  /** Selects these paths alone. */
  select(paths: string[]): void;
  /** Lists the folder shown again. */
  reload(): void;
  fail(err: unknown): void;
  ask(question: Confirm): void;
}

/**
 * What the file panel does to remote files besides transferring them: moving, renaming,
 * new folders, permissions and deleting (after asking), with the names being entered and the
 * entry whose permissions are being changed.
 */
export function useFileOps({ sessionId, cwd, listRef, select, reload, fail, ask }: Options) {
  const { t } = useTranslation();
  const sessionRef = useLatest(sessionId);
  const [renaming, setRenaming] = useState<{ path: string; value: string } | null>(null);
  const [newFolder, setNewFolder] = useState<string | null>(null);
  const [chmodTarget, setChmodTarget] = useState<FileEntry | null>(null);

  /** Moves remote `paths` into folder `dir`, then lists the folder shown again. */
  const move = async (paths: string[], dir: string) => {
    if (sessionId == null) return;
    for (const path of paths) {
      try {
        await sftp.rename(sessionId, path, joinPath(dir, basename(path)));
      } catch (err) {
        fail(err);
        break;
      }
    }
    reload();
  };

  const submitRename = async (e: FormEvent) => {
    e.preventDefault();
    if (!renaming || sessionId == null || !cwd) return;
    const name = renaming.value.trim();
    setRenaming(null);
    listRef.current?.focus();
    if (!name || name === basename(renaming.path)) return;
    try {
      const path = joinPath(cwd, name);
      await sftp.rename(sessionId, renaming.path, path);
      select([path]);
      reload();
    } catch (err) {
      fail(err);
    }
  };

  const submitNewFolder = async (e: FormEvent) => {
    e.preventDefault();
    const name = newFolder?.trim();
    setNewFolder(null);
    listRef.current?.focus();
    if (!name || sessionId == null || !cwd) return;
    try {
      const path = joinPath(cwd, name);
      await sftp.mkdir(sessionId, path);
      select([path]);
      reload();
    } catch (err) {
      fail(err);
    }
  };

  const chmod = async (entry: FileEntry, mode: number) => {
    setChmodTarget(null);
    if (sessionId == null) return;
    try {
      await sftp.chmod(sessionId, entry.path, mode);
      reload();
    } catch (err) {
      fail(err);
    }
  };

  const askRemove = (targets: FileEntry[]) => {
    if (targets.length === 0) return;
    const [first] = targets;
    ask({
      title: t("sftp.deleteTitle"),
      message:
        targets.length > 1
          ? t("sftp.deleteItemsMessage", { count: targets.length })
          : first.isDir && !first.isSymlink
            ? t("sftp.deleteFolderMessage", { name: first.name })
            : t("sftp.deleteFileMessage", { name: first.name }),
      confirmLabel: t("common.delete"),
      danger: true,
      // Through the connection there is when the user confirms: it may have been replaced.
      action: async () => {
        const current = sessionRef.current;
        if (current == null) return fail(notConnected(t("sftp.disconnected")));
        for (const entry of targets) {
          try {
            await sftp.remove(current, entry.path);
          } catch (err) {
            fail(err);
            break;
          }
        }
        reload();
      },
    });
  };

  const startRename = (entry: FileEntry) => setRenaming({ path: entry.path, value: entry.name });

  return {
    renaming,
    setRenaming,
    newFolder,
    setNewFolder,
    chmodTarget,
    setChmodTarget,
    move,
    submitRename,
    submitNewFolder,
    chmod,
    askRemove,
    startRename,
  };
}
