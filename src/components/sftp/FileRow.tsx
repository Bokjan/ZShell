import type { FormEvent, MouseEvent } from "react";
import { useTranslation } from "react-i18next";

import type { FileEntry } from "../../ipc/api";
import { formatMode } from "../../lib/format";
import { isComposing } from "../../lib/platform";
import { useLocale } from "../../hooks/locale";
import { useStableProps } from "../../hooks/stableProps";
import { FileIcon, FolderIcon } from "../icons";

/** What a row does: one object for every row, the same from render to render (`useStableHandlers`). */
export interface RowActions {
  onMouseDown(e: MouseEvent, entry: FileEntry): void;
  onClick(e: MouseEvent, entry: FileEntry): void;
  onDoubleClick(e: MouseEvent, entry: FileEntry): void;
  onContextMenu(e: MouseEvent, entry: FileEntry): void;
  /** The new name typed so far, while renaming the entry. */
  onRenameChange(entry: FileEntry, value: string): void;
  onRenameSubmit(e: FormEvent): void;
  /** Escape: back to the list, with nothing renamed. */
  onRenameCancel(): void;
  onRenameBlur(): void;
}

interface Props {
  entry: FileEntry;
  /** For the list's `aria-activedescendant`. */
  id: string;
  selected: boolean;
  /** Something dragged is over this folder. */
  dropTarget: boolean;
  /** The new name typed so far, while the entry is being renamed. */
  renaming: string | null;
  actions: RowActions;
}

const rowClass = (selected: boolean, dropTarget: boolean) =>
  [selected && "selected", dropTarget && "drop-target"].filter(Boolean).join(" ") || undefined;

/**
 * A row of the file list, its own component so that React Compiler memoizes it: the panel
 * renders again with each running transfer's progress, ten times a second, and the rows that
 * didn't change keep their elements and handlers (see "渲染与 memo" in docs/ARCHITECTURE.md).
 */
export function FileRow(props: Props) {
  useStableProps("FileRow", props, ["actions"]);
  const { entry, id, selected, dropTarget, renaming, actions } = props;
  const { t } = useTranslation();
  const { formatSize, formatTime } = useLocale();
  return (
    <tr
      id={id}
      role="row"
      aria-selected={selected}
      data-path={entry.path}
      data-drop-dir={entry.isDir ? entry.path : undefined}
      className={rowClass(selected, dropTarget)}
      onMouseDown={(e) => actions.onMouseDown(e, entry)}
      onClick={(e) => actions.onClick(e, entry)}
      onDoubleClick={(e) => actions.onDoubleClick(e, entry)}
      onContextMenu={(e) => actions.onContextMenu(e, entry)}
      title={`${entry.name}\n${formatMode(entry.permissions, entry.isDir, entry.isSymlink)}`}
    >
      <td role="gridcell">
        <div className="file-name">
          <span className={`file-icon${entry.isDir ? " folder" : ""}`} aria-hidden="true">
            {entry.isDir ? <FolderIcon /> : <FileIcon />}
          </span>
          {/* Before the name, which is the last child (see `.file-name`). */}
          {entry.isDir && <span className="visually-hidden">{t("sftp.folderKind")}</span>}
          {renaming !== null ? (
            <form onSubmit={actions.onRenameSubmit}>
              <input
                autoFocus
                value={renaming}
                onChange={(e) => actions.onRenameChange(entry, e.target.value)}
                onBlur={actions.onRenameBlur}
                onKeyDown={(e) => {
                  if (e.key !== "Escape" || isComposing(e)) return;
                  actions.onRenameCancel();
                }}
                onFocus={(e) =>
                  e.target.setSelectionRange(0, entry.name.lastIndexOf(".") > 0 ? entry.name.lastIndexOf(".") : entry.name.length)
                }
              />
            </form>
          ) : (
            <span className={entry.isSymlink ? "symlink" : undefined}>{entry.name}</span>
          )}
        </div>
      </td>
      <td className="file-size" role="gridcell">
        {entry.isDir ? "" : formatSize(entry.size)}
      </td>
      <td className="file-time" role="gridcell">
        {formatTime(entry.modified)}
      </td>
    </tr>
  );
}
