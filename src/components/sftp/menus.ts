import type { TFunction } from "i18next";

import type { FileEntry } from "../../lib/api";
import { isMac } from "../../lib/platform";
import type { MenuItem } from "../ContextMenu";

/** What the file panel's menus do. */
export interface FileActions {
  open(folder: FileEntry): void;
  download(targets: FileEntry[]): void;
  downloadTo(targets: FileEntry[]): void;
  openInEditor(file: FileEntry): void;
  rename(entry: FileEntry): void;
  changePermissions(entry: FileEntry): void;
  copyPaths(targets: FileEntry[]): void;
  remove(targets: FileEntry[]): void;
}

export interface FolderActions {
  uploadFiles(): void;
  uploadFolder(): void;
  newFolder(): void;
  refresh(): void;
  toggleHidden(): void;
}

/** The menu of selected entries; what needs the connection is disabled without it. */
export function fileMenu(t: TFunction, targets: FileEntry[], connected: boolean, actions: FileActions): MenuItem[] {
  const one = targets.length === 1 ? targets[0] : null;
  const offline = !connected;
  const items: MenuItem[] = [];
  if (one?.isDir) items.push({ label: t("sftp.open"), disabled: offline, onSelect: () => actions.open(one) });
  items.push(
    { label: t("sftp.download"), disabled: offline, onSelect: () => actions.download(targets) },
    { label: t("sftp.downloadTo"), disabled: offline, onSelect: () => actions.downloadTo(targets) },
  );
  if (one && !one.isDir) items.push({ label: t("sftp.openInEditor"), disabled: offline, onSelect: () => actions.openInEditor(one) });
  items.push("separator");
  if (one) {
    items.push(
      { label: t("sftp.rename"), shortcut: isMac ? undefined : "F2", disabled: offline, onSelect: () => actions.rename(one) },
      { label: t("sftp.changePermissions"), disabled: offline, onSelect: () => actions.changePermissions(one) },
    );
  }
  items.push(
    { label: targets.length > 1 ? t("sftp.copyPaths") : t("sftp.copyPath"), onSelect: () => actions.copyPaths(targets) },
    "separator",
    { label: t("sftp.delete"), shortcut: isMac ? "⌫" : "Del", danger: true, disabled: offline, onSelect: () => actions.remove(targets) },
  );
  return items;
}

/** The menu of the folder shown (nothing selected). */
export function folderMenu(t: TFunction, usable: boolean, showHidden: boolean, actions: FolderActions): MenuItem[] {
  return [
    { label: t("sftp.uploadFiles"), disabled: !usable, onSelect: actions.uploadFiles },
    { label: t("sftp.uploadFolder"), disabled: !usable, onSelect: actions.uploadFolder },
    { label: t("sftp.newFolder"), disabled: !usable, onSelect: actions.newFolder },
    "separator",
    { label: t("sftp.refresh"), disabled: !usable, onSelect: actions.refresh },
    { label: showHidden ? t("sftp.hideHidden") : t("sftp.showHidden"), onSelect: actions.toggleHidden },
  ];
}
