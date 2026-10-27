import { useEffect, useEffectEvent, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { announce } from "../lib/announce";
import { sftp, type FileEntry, type SessionId } from "../ipc/api";
import type { SftpDetail, SftpTraffic, TrafficMeter } from "../lib/traffic";
import { open } from "../ipc/fileDialog";
import { historyKey, historyKeyLabel } from "../lib/folderHistory";
import { isKey } from "../lib/keys";
import { filterEntries, sortedEntries, storeView, storedView, type FileView, type SortKey } from "../lib/fileList";
import { clicked, movedTo, NO_SELECTION, pressed, selectOnly, type Selection } from "../lib/fileSelection";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import { isComposing, isMac } from "../lib/platform";
import { useLocale } from "../hooks/locale";
import { useMouseNavigation } from "../hooks/mouseNavigation";
import { useStableHandlers } from "../hooks/stableHandlers";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { ErrorBanner } from "./ErrorMessage";
import { IconButton } from "./IconButton";
import { ArrowIcon, ChevronIcon, CloseIcon, EyeIcon, LinkIcon, RefreshIcon, SearchIcon, TerminalIcon } from "./icons";
import { ChmodDialog } from "./sftp/ChmodDialog";
import { FileRow, type RowActions } from "./sftp/FileRow";
import { fileMenu, folderMenu, type FileActions } from "./sftp/menus";
import { PathField } from "./sftp/PathField";
import { UploadButton } from "./sftp/UploadButton";
import { useConfirmQueue } from "./sftp/useConfirmQueue";
import { useFileOps } from "./sftp/useFileOps";
import { useSftpDragDrop } from "./sftp/useSftpDragDrop";
import { useShellDir, useShellFollow } from "./sftp/useShellFollow";
import { useSftpListing } from "./sftp/useSftpListing";
import { useRowWindow } from "./sftp/useRowWindow";
import { useTransfers } from "./sftp/useTransfers";
import { parentPath } from "../lib/remotePath";
import { TransferList } from "./TransferList";
import { TrafficLights, useTrafficText } from "./TrafficLights";

interface Props {
  /** The pane whose connection it shows, and whose terminal it may follow. */
  paneKey: number;
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether this panel is visible and should receive file drops. */
  active: boolean;
  /** What the connection's SFTP channels carry. */
  traffic: SftpTraffic | undefined;
  /** How many uploads and downloads are running, each time that changes. */
  onTransfers(count: number): void;
  /** How many files are open in a local editor, each time that changes. */
  onEdits(count: number): void;
  /** How many questions are waiting, each time that changes. */
  onQuestions(count: number): void;
}

interface Menu {
  x: number;
  y: number;
  items: MenuItem[];
}

/**
 * The file panel of a pane's SSH connection: the remote folder's entries (sorted, filtered,
 * selected as in a file manager), what can be done with them, and the transfers.
 */
export function SftpPanel({ paneKey, sessionId, connected, active, traffic, onTransfers, onEdits, onQuestions }: Props) {
  const { t } = useTranslation();
  const trafficText = useTrafficText();
  /** The tooltip of a channel's lights: `shared` when transfers go on the browsing channel. */
  const channelSummary = (title: string, meter: TrafficMeter<SftpDetail> | undefined, shared: boolean) => {
    const detail = meter?.detail;
    return [
      title,
      detail?.channel != null ? trafficText.channel(detail.connection, detail.channel) : t("traffic.notOpened"),
      ...(shared ? [t("traffic.sharedChannel")] : []),
      ...trafficText.counts(meter?.counts),
    ];
  };
  const [view, setView] = useState<FileView>(storedView);
  /** The name filter; null while the filter field is closed. */
  const [filter, setFilter] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection>(NO_SELECTION);
  const gridId = useId();
  const [menu, setMenu] = useState<Menu | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  /** The rows the last two presses in the list were on (null elsewhere). */
  const pressedRowsRef = useRef<[string | null, string | null]>([null, null]);

  const { startDir, syncedRef } = useShellDir(paneKey);
  const listing = useSftpListing(
    sessionId,
    connected,
    () => {
      setSelection(NO_SELECTION);
      setFilter(null);
    },
    startDir,
  );
  const { cwd, entries, loading, error, fail, load, reload, go, canGoBack, canGoForward } = listing;
  useMouseNavigation(panelRef, (forward) => go(forward ? 1 : -1));
  const shellFollow = useShellFollow({
    paneKey,
    sessionId,
    connected,
    active,
    follow: view.followTerminal,
    cwdRef: listing.cwdRef,
    syncedRef,
    load,
    setError: listing.setError,
  });
  const { confirm, count: questions, ask, answered } = useConfirmQueue();
  const reportQuestions = useEffectEvent(onQuestions);
  useEffect(() => reportQuestions(questions), [questions]);
  const transfers = useTransfers({
    sessionId,
    connected,
    active,
    cwdRef: listing.cwdRef,
    entries,
    load,
    ask,
    fail,
    onTransfers,
    onEdits,
  });

  // Sorted apart from the filter, which keeps their order: typing in the filter sorts nothing.
  const locale = useLocale();
  const { compareNames } = locale;
  const sorted = useMemo(() => sortedEntries(entries, view, compareNames), [entries, view, compareNames]);
  const visible = useMemo(() => filterEntries(sorted, filter ?? ""), [sorted, filter]);
  const rows = useMemo(() => visible.map((entry) => entry.path), [visible]);
  // What actions apply to: the selected entries that are shown, in the listed order.
  const selected = useMemo(() => visible.filter((entry) => selection.paths.has(entry.path)), [visible, selection]);
  const { cursor } = selection;

  const changeView = (patch: Partial<FileView>) => setView((current) => ({ ...current, ...patch }));
  // Remembered once changed, rather than in the updater, which React may call twice.
  useEffect(() => storeView(view), [view]);

  const select = (paths: string[]) => setSelection(selectOnly(paths));

  const fileOps = useFileOps({ sessionId, cwd, listRef, select, reload, fail, ask });
  const { renaming, setRenaming, newFolder, setNewFolder, chmodTarget, setChmodTarget, startRename, askRemove } = fileOps;

  const pickAndUpload = async (directory: boolean) => {
    if (!cwd) return;
    const picked = await open({ multiple: true, directory, title: directory ? t("sftp.chooseFolders") : t("sftp.chooseFiles") });
    if (picked) void transfers.upload(Array.isArray(picked) ? picked : [picked], cwd);
  };

  const drag = useSftpDragDrop({
    panelRef,
    listRef,
    active,
    sessionId,
    connected,
    cwdRef: listing.cwdRef,
    upload: transfers.upload,
    move: fileOps.move,
    transfers,
    fail,
  });
  const { dragOver, dropDir } = drag;

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

  const sortBy = (key: SortKey) => changeView(view.sort === key ? { descending: !view.descending } : { sort: key, descending: false });

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

  // Moved by a key: brought into view once drawn (a long list draws the cursor's row wherever it
  // is, but only after this render).
  const [cursorMoved, setCursorMoved] = useState(0);
  useLayoutEffect(() => {
    if (cursorMoved === 0 || !cursor) return;
    listRef.current?.querySelector(`tr[data-path="${CSS.escape(cursor)}"]`)?.scrollIntoView({ block: "nearest" });
  }, [cursorMoved]);

  const moveCursor = (index: number, extend: boolean) => {
    setSelection(movedTo(selection, rows, index, extend));
    setCursorMoved((count) => count + 1);
  };

  const onListKeyDown = (e: KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input")) return;
    const index = visible.findIndex((entry) => entry.path === cursor);
    const mod = isMod(e);
    const step = historyKey(e, isMac);
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
    else if (step) go(step);
    else if (e.key === "Escape") {
      if (filter !== null) closeFilter();
      else select([]);
    } else if (isKey(e, "a") && mod && !e.shiftKey && !e.altKey) select(visible.map((entry) => entry.path));
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

  // The same from render to render, so that a row renders again only when it changes.
  const rowActions = useStableHandlers<RowActions>({
    onMouseDown: onRowMouseDown,
    onClick: onRowClick,
    onDoubleClick: (e, entry) => isDoubleClick(e, entry) && activate(entry),
    onContextMenu: onRowContextMenu,
    onRenameChange: (entry, value) => setRenaming({ path: entry.path, value }),
    onRenameSubmit: (e) => void fileOps.submitRename(e),
    onRenameCancel: () => {
      setRenaming(null);
      listRef.current?.focus();
    },
    onRenameBlur: () => setRenaming(null),
  });

  const cursorIndex = visible.findIndex((entry) => entry.path === cursor);
  const renamingIndex = renaming ? visible.findIndex((entry) => entry.path === renaming.path) : -1;
  // A long list draws the rows in view, and those of the cursor (which screen readers are
  // pointed at) and of the name being edited wherever they are.
  const rowWindow = useRowWindow(listRef, visible.length, [cursorIndex, renamingIndex]);

  if (!connected && cwd == null) {
    return (
      <div className="sftp-panel" ref={panelRef}>
        <div className="sftp-empty">{t("sftp.notConnectedYet")}</div>
      </div>
    );
  }

  // One handler for the three, rather than a new one for each as the panel renders again.
  const onSortClick = (e: MouseEvent<HTMLButtonElement>) => sortBy(e.currentTarget.dataset.sort as SortKey);
  const sortHeader = (key: SortKey, label: string, className?: string) => (
    <th
      className={className}
      role="columnheader"
      aria-sort={view.sort === key ? (view.descending ? "descending" : "ascending") : undefined}
    >
      <button type="button" className="sort-button" data-sort={key} onClick={onSortClick}>
        <span className="sort-label">{label}</span>
        {view.sort === key && <ChevronIcon direction={view.descending ? "down" : "up"} size={8} />}
      </button>
    </th>
  );

  const parent = cwd && cwd !== "/" ? parentPath(cwd) : null;
  // Counted from 1, after the header row and the new folder's, if any (see `aria-rowcount`).
  const firstRowIndex = newFolder != null ? 3 : 2;
  const fileRow = (index: number) => {
    const entry = visible[index];
    return (
      <FileRow
        key={entry.path}
        entry={entry}
        id={`${gridId}-${index}`}
        rowIndex={rowWindow ? firstRowIndex + index : undefined}
        selected={selection.paths.has(entry.path)}
        dropTarget={dropDir === entry.path}
        renaming={renaming?.path === entry.path ? renaming.value : null}
        actions={rowActions}
        t={t}
        locale={locale}
      />
    );
  };
  const hiddenCount = view.showHidden ? 0 : entries.filter((entry) => entry.name.startsWith(".")).length;

  return (
    <div className={`sftp-panel${dragOver && !dropDir ? " drag-over" : ""}`} ref={panelRef}>
      <div className="sftp-toolbar">
        <IconButton
          className="icon-button"
          label={t("sftp.back", { shortcut: historyKeyLabel(-1, isMac, t("keys.alt")) })}
          disabled={!connected || !canGoBack}
          onClick={() => go(-1)}
        >
          <ArrowIcon direction="left" />
        </IconButton>
        <IconButton
          className="icon-button"
          label={t("sftp.forward", { shortcut: historyKeyLabel(1, isMac, t("keys.alt")) })}
          disabled={!connected || !canGoForward}
          onClick={() => go(1)}
        >
          <ArrowIcon direction="right" />
        </IconButton>
        <IconButton
          className={`icon-button${parent && dropDir === parent ? " drop-target" : ""}`}
          label={t("sftp.parentFolder")}
          disabled={!parent}
          data-drop-dir={parent ?? undefined}
          onClick={() => parent && void load(parent)}
        >
          <ArrowIcon direction="up" />
        </IconButton>
        <div className="sftp-toolbar-rest">
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
            <PathField
              sessionId={sessionId}
              value={listing.pathInput}
              onChange={listing.setPathInput}
              onGo={(path) => void load(path)}
              onEditing={listing.setEditing}
              showHidden={view.showHidden}
            />
          </form>
          {/* Where dragged files go, over the refresh button and the path: a place for the
            folder shown, which the list's folders may leave none of, that doesn't move the
            list. The parent folder's button before them is a place of its own. */}
          {dragOver && (
            <div className={`sftp-drop-zone${dropDir ? "" : " drop-target"}`}>
              <span>{t("sftp.dropHint", { path: dropDir ?? cwd ?? "" })}</span>
            </div>
          )}
        </div>
      </div>
      <div className="sftp-actions">
        <UploadButton disabled={!connected || !cwd} onUpload={(directory) => void pickAndUpload(directory)} />
        <button disabled={!connected || !cwd} onClick={() => setNewFolder("")}>
          {t("sftp.newFolder")}
        </button>
        <div className="sftp-view-buttons">
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
          <IconButton
            className="icon-button"
            label={t("sftp.goToTerminalFolder")}
            disabled={!connected}
            onClick={() => void shellFollow.goToShellDir()}
          >
            <TerminalIcon />
          </IconButton>
          <IconButton
            className={`icon-button${view.followTerminal ? " on" : ""}`}
            label={t("sftp.followTerminal")}
            aria-pressed={view.followTerminal}
            onClick={() => {
              const follow = !view.followTerminal;
              changeView({ followTerminal: follow });
              if (follow && connected) void shellFollow.goToShellDir();
            }}
          >
            <LinkIcon />
          </IconButton>
        </div>
        <TrafficLights
          className="sftp-traffic"
          source={traffic?.browse}
          summary={() => channelSummary(t("traffic.browse"), traffic?.browse, false)}
        />
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
            autoCapitalize="off"
            autoCorrect="off"
          />
          <IconButton className="icon-button" label={t("sftp.clearFilter")} onClick={closeFilter}>
            <CloseIcon size={12} />
          </IconButton>
        </div>
      )}

      {error && <ErrorBanner onDismiss={() => listing.setError(null)}>{error}</ErrorBanner>}
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
        aria-rowcount={rowWindow ? firstRowIndex - 1 + visible.length : undefined}
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
            <tr role="row" aria-rowindex={rowWindow ? 1 : undefined}>
              {sortHeader("name", t("sftp.columnName"))}
              {sortHeader("size", t("sftp.columnSize"), "file-size")}
              {sortHeader("modified", t("sftp.columnModified"))}
            </tr>
          </thead>
          <tbody role="rowgroup">
            {newFolder != null && (
              <tr role="row" aria-rowindex={rowWindow ? 2 : undefined}>
                <td colSpan={3} role="gridcell">
                  <form onSubmit={fileOps.submitNewFolder}>
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
            {rowWindow
              ? // One list, keyed by path: rows stay as gaps come and go before them.
                rowWindow.segments.flatMap((segment, i) =>
                  segment.kind === "rows"
                    ? Array.from({ length: segment.end - segment.start }, (_, j) => fileRow(segment.start + j))
                    : [
                        <tr key={`gap-${i}`} className="sftp-gap" aria-hidden="true">
                          <td colSpan={3} style={{ height: segment.rows * rowWindow.rowHeight }} />
                        </tr>,
                      ],
                )
              : visible.map((_, index) => fileRow(index))}
          </tbody>
        </table>
        {visible.length === 0 && !loading && cwd && (
          <div className="sftp-empty">
            {filter ? t("sftp.noMatches") : hiddenCount > 0 ? t("sftp.onlyHidden", { count: hiddenCount }) : t("sftp.emptyFolder")}
          </div>
        )}
      </div>
      {selected.length > 1 && <div className="sftp-status">{t("sftp.selectedCount", { count: selected.length })}</div>}

      <TransferList
        transfers={transfers.transfers}
        progress={transfers.progress}
        traffic={
          <TrafficLights
            source={traffic?.transfer}
            rates
            summary={() => {
              const channel = traffic?.transfer.detail?.channel;
              return channelSummary(
                t("traffic.transfers"),
                traffic?.transfer,
                channel != null && channel === traffic?.browse.detail?.channel,
              );
            }}
          />
        }
        connected={connected}
        onCancel={(id) => void sftp.cancel(id)}
        onDelete={transfers.deleteLeft}
        onDismiss={transfers.remove}
        onClearFinished={transfers.clearFinished}
        onReopen={(id) => void sftp.editReopen(id).catch(fail)}
        onStopEditing={transfers.stopEditing}
      />

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {chmodTarget && (
        <ChmodDialog entry={chmodTarget} onSubmit={(mode) => void fileOps.chmod(chmodTarget, mode)} onClose={() => setChmodTarget(null)} />
      )}
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
