import { useId, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";

import { announce } from "../lib/announce";
import { sftp, type FileEntry, type SessionId } from "../lib/api";
import { storeView, storedView, visibleEntries, type FileView, type SortKey } from "../lib/fileList";
import { clicked, movedTo, NO_SELECTION, pressed, selectOnly, type Selection } from "../lib/fileSelection";
import { basename, formatMode, formatSize, formatTime } from "../lib/format";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import { isComposing, isMac } from "../lib/platform";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { ErrorBanner } from "./ErrorMessage";
import { IconButton } from "./IconButton";
import { ArrowIcon, ChevronIcon, CloseIcon, EyeIcon, FileIcon, FolderIcon, RefreshIcon, SearchIcon } from "./icons";
import { ChmodDialog } from "./sftp/ChmodDialog";
import { fileMenu, folderMenu, type FileActions } from "./sftp/menus";
import { useConfirmQueue } from "./sftp/useConfirmQueue";
import { useSftpDragDrop } from "./sftp/useSftpDragDrop";
import { useSftpListing } from "./sftp/useSftpListing";
import { parentPath, useTransfers } from "./sftp/useTransfers";
import { TransferList } from "./TransferList";

interface Props {
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether this panel is visible and should receive file drops. */
  active: boolean;
  /** How many uploads and downloads are running, each time that changes. */
  onTransfers(count: number): void;
}

interface Menu {
  x: number;
  y: number;
  items: MenuItem[];
}

const joinPath = (dir: string, name: string) => (dir.endsWith("/") ? dir + name : `${dir}/${name}`);

/**
 * The file panel of a pane's SSH connection: the remote folder's entries (sorted, filtered,
 * selected as in a file manager), what can be done with them, and the transfers.
 */
export function SftpPanel({ sessionId, connected, active, onTransfers }: Props) {
  const { t } = useTranslation();
  const [view, setView] = useState<FileView>(storedView);
  /** The name filter; null while the filter field is closed. */
  const [filter, setFilter] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection>(NO_SELECTION);
  const gridId = useId();
  const [renaming, setRenaming] = useState<{ path: string; value: string } | null>(null);
  const [newFolder, setNewFolder] = useState<string | null>(null);
  const [chmodTarget, setChmodTarget] = useState<FileEntry | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  /** The rows the last two presses in the list were on (null elsewhere). */
  const pressedRowsRef = useRef<[string | null, string | null]>([null, null]);

  const listing = useSftpListing(sessionId, connected, () => {
    setSelection(NO_SELECTION);
    setFilter(null);
  });
  const { cwd, entries, loading, error, fail, load, reload } = listing;
  const { confirm, ask, answered } = useConfirmQueue();
  const transfers = useTransfers({ sessionId, connected, active, cwdRef: listing.cwdRef, entries, load, ask, fail, onTransfers });

  const visible = useMemo(() => visibleEntries(entries, view, filter ?? ""), [entries, view, filter]);
  const rows = useMemo(() => visible.map((entry) => entry.path), [visible]);
  // What actions apply to: the selected entries that are shown, in the listed order.
  const selected = useMemo(() => visible.filter((entry) => selection.paths.has(entry.path)), [visible, selection]);
  const { cursor } = selection;

  const changeView = (patch: Partial<FileView>) =>
    setView((current) => {
      const next = { ...current, ...patch };
      storeView(next);
      return next;
    });

  const select = (paths: string[]) => setSelection(selectOnly(paths));

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

  const drag = useSftpDragDrop({
    panelRef,
    active,
    sessionId,
    connected,
    cwdRef: listing.cwdRef,
    upload: transfers.upload,
    move,
    transfers,
    fail,
  });
  const { dragOver, dropDir } = drag;

  const pickAndUpload = async (directory: boolean) => {
    if (!cwd) return;
    const picked = await open({ multiple: true, directory, title: directory ? t("sftp.chooseFolders") : t("sftp.chooseFiles") });
    if (picked) void transfers.upload(Array.isArray(picked) ? picked : [picked], cwd);
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
      action: async () => {
        if (sessionId == null) return;
        for (const entry of targets) {
          try {
            await sftp.remove(sessionId, entry.path);
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

  const activate = (entry: FileEntry) => (entry.isDir ? void load(entry.path) : transfers.download([entry]));

  const openFilter = (text = "") => {
    setFilter((current) => (current ?? "") + text);
    // Focused after it renders, also when it was already open.
    setTimeout(() => filterRef.current?.focus());
  };

  const closeFilter = () => {
    setFilter(null);
    listRef.current?.focus();
  };

  const sortBy = (key: SortKey) =>
    changeView(view.sort === key ? { descending: !view.descending } : { sort: key, descending: false });

  const isMod = (e: { metaKey: boolean; ctrlKey: boolean }) => (isMac ? e.metaKey : e.ctrlKey);

  const modifiers = (e: MouseEvent) => ({ shift: e.shiftKey, mod: isMod(e) });

  const onRowMouseDown = (e: MouseEvent, entry: FileEntry) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("input")) return;
    const next = pressed(selection, rows, entry.path, modifiers(e));
    setSelection(next);
    // Dragged out: the selection if the row is in it, else the row.
    if (!e.shiftKey && !isMod(e)) drag.press(e, () => (next.paths.size > 1 ? visible.filter((v) => next.paths.has(v.path)) : [entry]));
  };

  // WebKit also counts a click on the header (or another row) followed quickly by one on a
  // row as a double click.
  const isDoubleClick = (e: MouseEvent, entry: FileEntry) =>
    !e.shiftKey && !isMod(e) && pressedRowsRef.current.every((path) => path === entry.path);

  const onRowClick = (e: MouseEvent, entry: FileEntry) => setSelection(clicked(selection, entry.path, modifiers(e)));

  const fileActions: FileActions = {
    open: (folder) => void load(folder.path),
    download: (targets) => transfers.download(targets),
    downloadTo: (targets) => void transfers.downloadTo(targets),
    openInEditor: (file) => void transfers.openInEditor(file),
    rename: startRename,
    changePermissions: setChmodTarget,
    copyPaths: (targets) => void writeText(targets.map((e) => e.path).join("\n")).then(() => announce(t("announce.copied")), fail),
    remove: askRemove,
  };
  const filesMenu = (targets: FileEntry[]) => fileMenu(t, targets, connected, fileActions);
  const shownFolderMenu = () =>
    folderMenu(t, connected && !!cwd, view.showHidden, {
      uploadFiles: () => void pickAndUpload(false),
      uploadFolder: () => void pickAndUpload(true),
      newFolder: () => setNewFolder(""),
      refresh: reload,
      toggleHidden: () => changeView({ showHidden: !view.showHidden }),
    });

  // The menu key and Shift+F10 open the selection's menu below the cursor's row, or the
  // folder's with nothing selected.
  const openMenuFromKeyboard = () => {
    openedByMenuKey();
    const row = cursor ? listRef.current?.querySelector(`tr[data-path="${CSS.escape(cursor)}"]`) : null;
    const rect = (row ?? listRef.current!).getBoundingClientRect();
    setMenu({ x: rect.left + 24, y: row ? rect.bottom : rect.top, items: selected.length > 0 ? filesMenu(selected) : shownFolderMenu() });
  };

  const onRowContextMenu = (e: MouseEvent, entry: FileEntry) => {
    if (followsMenuKey(e)) return;
    e.preventDefault();
    e.stopPropagation();
    const targets = selection.paths.has(entry.path) ? selected : [entry];
    if (!selection.paths.has(entry.path)) select([entry.path]);
    setMenu({ x: e.clientX, y: e.clientY, items: filesMenu(targets) });
  };

  const onListContextMenu = (e: MouseEvent) => {
    // It would clear the selection and open the folder's menu.
    if (followsMenuKey(e)) return;
    e.preventDefault();
    select([]);
    setMenu({ x: e.clientX, y: e.clientY, items: shownFolderMenu() });
  };

  const moveCursor = (index: number, extend: boolean) => {
    const next = movedTo(selection, rows, index, extend);
    setSelection(next);
    if (next.cursor) listRef.current?.querySelector(`tr[data-path="${CSS.escape(next.cursor)}"]`)?.scrollIntoView({ block: "nearest" });
  };

  const onListKeyDown = (e: KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input")) return;
    const index = visible.findIndex((entry) => entry.path === cursor);
    const mod = isMod(e);
    let handled = true;
    if (e.key === "ArrowDown") moveCursor(index < 0 ? 0 : index + 1, e.shiftKey);
    else if (e.key === "ArrowUp") moveCursor(index < 0 ? visible.length - 1 : index - 1, e.shiftKey);
    else if (e.key === "Home") moveCursor(0, e.shiftKey);
    else if (e.key === "End") moveCursor(visible.length - 1, e.shiftKey);
    else if (e.key === "Enter" && !mod) {
      if (selected.length === 1) activate(selected[0]);
      else transfers.download(selected);
    } else if ((e.key === "Delete" || e.key === "Backspace") && connected) askRemove(selected);
    else if (e.key === "F2" && selected.length === 1 && connected) startRename(selected[0]);
    else if (isMenuKey(e)) openMenuFromKeyboard();
    else if (e.key === "Escape") {
      if (filter !== null) closeFilter();
      else select([]);
    } else if (e.code === "KeyA" && mod && !e.shiftKey && !e.altKey) select(visible.map((entry) => entry.path));
    else if (e.key.length === 1 && e.key !== " " && !e.metaKey && !e.ctrlKey && !e.altKey) openFilter(e.key);
    else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  const onFilterKeyDown = (e: KeyboardEvent) => {
    if (isComposing(e)) return;
    if (e.key === "Escape") closeFilter();
    else if (e.key === "ArrowDown" || e.key === "Enter") {
      listRef.current?.focus();
      if (selected.length === 0) moveCursor(0, false);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  if (!connected && cwd == null) {
    return (
      <div className="sftp-panel" ref={panelRef}>
        <div className="sftp-empty">{t("sftp.notConnectedYet")}</div>
      </div>
    );
  }

  const sortHeader = (key: SortKey, label: string, className?: string) => (
    <th
      className={className}
      role="columnheader"
      aria-sort={view.sort === key ? (view.descending ? "descending" : "ascending") : undefined}
    >
      <button type="button" className="sort-button" onClick={() => sortBy(key)}>
        <span className="sort-label">{label}</span>
        {view.sort === key && <ChevronIcon direction={view.descending ? "down" : "up"} size={8} />}
      </button>
    </th>
  );

  const parent = cwd && cwd !== "/" ? parentPath(cwd) : null;
  const cursorIndex = visible.findIndex((entry) => entry.path === cursor);
  const hiddenCount = view.showHidden ? 0 : entries.filter((entry) => entry.name.startsWith(".")).length;

  return (
    <div className={`sftp-panel${dragOver && !dropDir ? " drag-over" : ""}`} ref={panelRef}>
      <div className="sftp-toolbar">
        <IconButton
          className={`icon-button${parent && dropDir === parent ? " drop-target" : ""}`}
          label={t("sftp.parentFolder")}
          disabled={!parent}
          data-drop-dir={parent ?? undefined}
          onClick={() => parent && void load(parent)}
        >
          <ArrowIcon direction="up" />
        </IconButton>
        <IconButton className="icon-button" label={t("sftp.refresh")} disabled={!cwd} onClick={reload}>
          <RefreshIcon />
        </IconButton>
        <form
          className="sftp-path"
          onSubmit={(e) => {
            e.preventDefault();
            void load(listing.pathInput.trim() || ".");
          }}
        >
          <input value={listing.pathInput} onChange={(e) => listing.setPathInput(e.target.value)} spellCheck={false} title={t("sftp.pathHint")} />
        </form>
        <IconButton
          className={`icon-button${filter !== null ? " on" : ""}`}
          label={t("sftp.filter")}
          aria-pressed={filter !== null}
          onClick={() => (filter === null ? openFilter() : closeFilter())}
        >
          <SearchIcon />
        </IconButton>
        <IconButton
          className="icon-button"
          label={view.showHidden ? t("sftp.hideHiddenFiles") : t("sftp.showHiddenFiles")}
          onClick={() => changeView({ showHidden: !view.showHidden })}
        >
          <EyeIcon crossed={!view.showHidden} />
        </IconButton>
      </div>
      <div className="sftp-actions">
        <button disabled={!connected || !cwd} onClick={() => pickAndUpload(false)}>
          {t("sftp.uploadFiles")}
        </button>
        <button disabled={!connected || !cwd} onClick={() => pickAndUpload(true)}>
          {t("sftp.uploadFolder")}
        </button>
        <button disabled={!connected || !cwd} onClick={() => setNewFolder("")}>
          {t("sftp.newFolder")}
        </button>
      </div>
      {filter !== null && (
        <div className="sftp-filter">
          <SearchIcon size={12} />
          <input
            ref={filterRef}
            value={filter}
            placeholder={t("sftp.filterPlaceholder")}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={onFilterKeyDown}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
          <IconButton className="icon-button" label={t("sftp.clearFilter")} onClick={closeFilter}>
            <CloseIcon size={12} />
          </IconButton>
        </div>
      )}

      {error && (
        <ErrorBanner onDismiss={() => listing.setError(null)}>{error}</ErrorBanner>
      )}
      {!connected && <div className="panel-error">{t("sftp.disconnected")}</div>}

      {/* A grid for screen readers: the list keeps the focus and points them at the cursor's row. */}
      <div
        ref={listRef}
        className={`sftp-list${loading ? " loading" : ""}`}
        role="grid"
        aria-label={t("sftp.listLabel")}
        aria-multiselectable="true"
        aria-busy={loading}
        aria-activedescendant={cursorIndex >= 0 ? `${gridId}-${cursorIndex}` : undefined}
        tabIndex={0}
        onKeyDown={onListKeyDown}
        onMouseDownCapture={(e) => {
          const row = (e.target as HTMLElement).closest<HTMLElement>("tr[data-path]");
          pressedRowsRef.current = [pressedRowsRef.current[1], row?.dataset.path ?? null];
        }}
        onMouseDown={(e) => {
          if (e.target === e.currentTarget || (e.target as HTMLElement).closest(".sftp-empty")) select([]);
        }}
        onContextMenu={onListContextMenu}
      >
        <table role="presentation">
          <colgroup>
            <col />
            <col className="col-size" />
            <col className="col-time" />
          </colgroup>
          <thead role="rowgroup">
            <tr role="row">
              {sortHeader("name", t("sftp.columnName"))}
              {sortHeader("size", t("sftp.columnSize"), "file-size")}
              {sortHeader("modified", t("sftp.columnModified"))}
            </tr>
          </thead>
          <tbody role="rowgroup">
            {newFolder != null && (
              <tr role="row">
                <td colSpan={3} role="gridcell">
                  <form onSubmit={submitNewFolder}>
                    <input
                      autoFocus
                      placeholder={t("sftp.newFolderPlaceholder")}
                      value={newFolder}
                      onChange={(e) => setNewFolder(e.target.value)}
                      onBlur={() => setNewFolder(null)}
                      onKeyDown={(e) => e.key === "Escape" && !isComposing(e) && setNewFolder(null)}
                    />
                  </form>
                </td>
              </tr>
            )}
            {visible.map((entry, index) => (
              <tr
                key={entry.path}
                id={`${gridId}-${index}`}
                role="row"
                aria-selected={selection.paths.has(entry.path)}
                data-path={entry.path}
                data-drop-dir={entry.isDir ? entry.path : undefined}
                className={
                  [selection.paths.has(entry.path) && "selected", dropDir === entry.path && "drop-target"].filter(Boolean).join(" ") ||
                  undefined
                }
                onMouseDown={(e) => onRowMouseDown(e, entry)}
                onClick={(e) => onRowClick(e, entry)}
                onDoubleClick={(e) => isDoubleClick(e, entry) && activate(entry)}
                onContextMenu={(e) => onRowContextMenu(e, entry)}
                title={`${entry.name}\n${formatMode(entry.permissions, entry.isDir, entry.isSymlink)}`}
              >
                <td role="gridcell">
                  <div className="file-name">
                    <span className={`file-icon${entry.isDir ? " folder" : ""}`} aria-hidden="true">
                      {entry.isDir ? <FolderIcon /> : <FileIcon />}
                    </span>
                    {/* Before the name, which is the last child (see `.file-name`). */}
                    {entry.isDir && <span className="visually-hidden">{t("sftp.folderKind")}</span>}
                    {renaming?.path === entry.path ? (
                      <form onSubmit={submitRename}>
                        <input
                          autoFocus
                          value={renaming.value}
                          onChange={(e) => setRenaming({ path: entry.path, value: e.target.value })}
                          onBlur={() => setRenaming(null)}
                          onKeyDown={(e) => {
                            if (e.key !== "Escape" || isComposing(e)) return;
                            setRenaming(null);
                            listRef.current?.focus();
                          }}
                          onFocus={(e) => e.target.setSelectionRange(0, entry.name.lastIndexOf(".") > 0 ? entry.name.lastIndexOf(".") : entry.name.length)}
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
            ))}
          </tbody>
        </table>
        {visible.length === 0 && !loading && cwd && (
          <div className="sftp-empty">
            {filter
              ? t("sftp.noMatches")
              : hiddenCount > 0
                ? t("sftp.onlyHidden", { count: hiddenCount })
                : t("sftp.emptyFolder")}
          </div>
        )}
      </div>
      {selected.length > 1 && <div className="sftp-status">{t("sftp.selectedCount", { count: selected.length })}</div>}

      <TransferList
        transfers={transfers.transfers}
        onCancel={(id) => void sftp.cancel(id)}
        onDismiss={transfers.remove}
        onClearFinished={transfers.clearFinished}
        onReopen={(id) => void sftp.editReopen(id).catch(fail)}
        onStopEditing={transfers.stopEditing}
      />

      {dragOver && (
        <div className="drop-hint">{t("sftp.dropHint", { path: dropDir ?? cwd ?? "" })}</div>
      )}

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {chmodTarget && <ChmodDialog entry={chmodTarget} onSubmit={(mode) => void chmod(chmodTarget, mode)} onClose={() => setChmodTarget(null)} />}
      {confirm && (
        <ConfirmDialog
          key={confirm.id}
          title={confirm.title}
          message={confirm.message}
          confirmLabel={confirm.confirmLabel}
          danger={confirm.danger}
          onConfirm={() => {
            answered();
            confirm.action();
          }}
          onCancel={answered}
        />
      )}
    </div>
  );
}
