import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import type { AppMenuButton } from "../app/useAppMenu";
import type { Release } from "../ipc/bindings";
import { announce } from "../lib/announce";
import { duplicateProfile, errorMessage, logs, tree, type Folder, type Profile, type TreeItem } from "../ipc/api";
import { directory, recentProfiles as recentOf } from "../lib/directory";
import { useDirectory } from "../hooks/stores";
import { usePressDrag } from "../hooks/drag";
import { open as openDialog } from "../ipc/fileDialog";
import { clampIndex, navigateList } from "../lib/listNavigation";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import { matches } from "../lib/keymap";
import { isComposing, isMac } from "../lib/platform";
import {
  address,
  folderPath,
  parseQuickConnect,
  rowKey,
  search,
  storeCollapsed,
  storedCollapsed,
  treeRows,
  type QuickTarget,
  type Row,
} from "../lib/sessions";
import { useSettings } from "../hooks/settings";
import { useShortcuts } from "../hooks/shortcuts";
import { dropMove, dropOnRow, sameDrop, type Drop } from "../lib/treeDrop";
import { useSidebarWidth } from "./useSidebarWidth";
import { DRAG_REGION } from "../hooks/titleBar";
import appIcon from "../../src-tauri/icons/source/icon.svg";
import { useStableHandlers } from "../hooks/stableHandlers";
import { useStableProps } from "../hooks/stableProps";
import { useLocale } from "../hooks/locale";
import { ChevronIcon, DotIcon, ImportExportIcon, PlusIcon, ProtocolIcon, SettingsIcon } from "./icons";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { ErrorBanner } from "./ErrorMessage";
import { FolderRow, INDENT, SessionRow, type RowActions } from "./sidebar/TreeRows";
import { IconButton } from "./IconButton";
import { Separator } from "./Separator";
import { ExportDialog } from "./ExportDialog";
import { SessionImportDialog } from "./SessionImportDialog";

/**
 * How a session is opened: switch to a pane it already has, always in a new tab, or in a
 * new pane right of or below the focused one.
 */
export type OpenMode = "connect" | "newTab" | "splitRight" | "splitDown" | "newWindow";

/** What the sidebar asks of the app. One object that stays the same (see `useStableHandlers`). */
export interface SidebarHandlers {
  onOpen(profile: Profile, mode: OpenMode): void;
  onOpenAll(folder: Folder): void;
  onQuickConnect(target: QuickTarget): void;
  onEdit(profile: Profile): void;
  /** Deletes the session once the user confirms; rejects if that fails. */
  onDelete(profile: Profile): Promise<boolean>;
  /** Creates a session in `folder` (undefined: top level). */
  onNew(folder: string | undefined): void;
  onImportSshConfig(): void;
  onSettings(): void;
}

interface Props {
  /** Changes when the search shortcut is pressed, to focus the search box. */
  searchFocusKey: number;
  handlers: SidebarHandlers;
  /** The app menu, opened by the app's icon and name outside macOS. */
  appMenu: AppMenuButton;
  appMenuOpen: boolean;
  /** A newer version, which marks the app menu's button, and on macOS the settings button, where the check is. */
  update: Release | null;
}

/** How far the pointer moves before a press on a row becomes a drag. */
const DRAG_THRESHOLD = 4;

/**
 * The saved sessions and folders, with search and quick connect. It doesn't depend on the tabs,
 * so it shouldn't render again as they change (titles change several times a second while a
 * program shows a spinner): its handlers come as one stable object, checked in development.
 */
export function Sidebar(props: Props) {
  useStableProps("Sidebar", props, ["handlers", "appMenu"]);
  const { searchFocusKey, handlers, appMenu, appMenuOpen, update } = props;
  // Taken apart here: React Compiler treats an object holding a ref as a ref, and reading any of
  // it while rendering would keep the whole component from being compiled.
  const { ref: appMenuRef, toggle: toggleAppMenu } = appMenu;
  const { onOpen, onOpenAll, onQuickConnect, onEdit, onDelete, onNew, onImportSshConfig, onSettings } = handlers;
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const { settings } = useSettings();
  const state = useDirectory();
  const { profiles, folders } = state;
  const asideRef = useRef<HTMLElement>(null);
  const { width, widthPercent, startResize, nudgeWidth, resetWidth } = useSidebarWidth(asideRef);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  // Highlighted search result (index into results, then the quick connect line).
  const [highlight, setHighlight] = useState(0);
  const listId = useId();
  /** A row's element id, for `aria-activedescendant`. */
  const rowId = (key: string) => `${listId}-${key}`;
  const [selected, setSelected] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(storedCollapsed);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  // A session whose logs are about to be deleted, with how many it has.
  const [deletingLogs, setDeletingLogs] = useState<{ profile: Profile; count: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ item: TreeItem; drop: Drop | null } | null>(null);
  // A sessions file being imported.
  const [importing, setImporting] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  // Not `state`, which also changes as sessions are opened (the recent ones).
  const sessionTree = { profiles, folders, order: state.order };
  const rows = treeRows(sessionTree, collapsed);
  const searching = query.trim() !== "";
  const results = searching ? search(query, sessionTree) : [];
  const quick = searching && results.length === 0 ? parseQuickConnect(query) : null;
  // The search results, then the quick connect line.
  const resultCount = results.length + (quick ? 1 : 0);
  const highlighted = clampIndex(highlight, resultCount);
  const recentProfiles = settings.sidebar.showRecent ? recentOf(state) : [];

  useEffect(() => {
    if (searchFocusKey === 0) return;
    searchRef.current?.focus();
    searchRef.current?.select();
  }, [searchFocusKey]);

  useEffect(() => setHighlight(0), [query]);

  const fail = (e: unknown) => setError(errorMessage(e));
  const run = (action: Promise<unknown>) => void directory.changed(action).catch(fail);

  const setFolderCollapsed = (id: string, value: boolean) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (value) next.add(id);
      else next.delete(id);
      storeCollapsed(next);
      return next;
    });

  /** The folder new things go in: the selected folder, or the selected session's. */
  const currentFolder = () => {
    if (selected?.startsWith("f:")) return selected.slice(2);
    if (selected?.startsWith("p:")) return profiles.find((p) => p.id === selected.slice(2))?.folder;
    return undefined;
  };

  const createFolder = (parent: string | undefined) => {
    directory.changed(tree.saveFolder({ id: "", name: t("sidebar.newFolderName"), parent })).then((folder) => {
      if (parent) setFolderCollapsed(parent, false);
      setSelected(`f:${folder.id}`);
      setRenaming(folder.id);
    }, fail);
  };

  const finishRename = (folder: Folder, name: string | null) => {
    setRenaming(null);
    listRef.current?.focus();
    if (name !== null && name !== folder.name) run(tree.saveFolder({ ...folder, name }));
  };

  const duplicate = (profile: Profile) => {
    directory.changed(duplicateProfile(profile.id, t("sidebar.copyName", { name: profile.name }))).then((saved) => {
      setSelected(`p:${saved.id}`);
    }, fail);
  };

  const profileMenu = (profile: Profile): MenuItem[] => [
    { label: t("sidebar.connect"), onSelect: () => onOpen(profile, "connect") },
    { label: t("sidebar.connectSplitRight"), onSelect: () => onOpen(profile, "splitRight") },
    { label: t("sidebar.connectSplitDown"), onSelect: () => onOpen(profile, "splitDown") },
    { label: t("sidebar.connectNewWindow"), onSelect: () => onOpen(profile, "newWindow") },
    "separator",
    { label: t("sidebar.edit"), onSelect: () => onEdit(profile) },
    { label: t("sidebar.duplicate"), onSelect: () => duplicate(profile) },
    {
      label: t("sidebar.copyAddress"),
      onSelect: () => void writeText(address(profile.connection)).then(() => announce(t("announce.copied")), fail),
    },
    "separator",
    {
      label: t("sidebar.deleteLogs"),
      onSelect: () => logs.count(profile.id).then((count) => setDeletingLogs({ profile, count }), fail),
    },
    { label: t("sidebar.delete"), danger: true, onSelect: () => void onDelete(profile).catch(fail) },
  ];

  const folderMenu = (folder: Folder, count: number): MenuItem[] => [
    { label: t("sidebar.openAll"), disabled: count === 0, onSelect: () => onOpenAll(folder) },
    "separator",
    { label: t("sidebar.newSessionHere"), onSelect: () => onNew(folder.id) },
    { label: t("sidebar.newFolderHere"), onSelect: () => createFolder(folder.id) },
    "separator",
    { label: t("sidebar.rename"), onSelect: () => setRenaming(folder.id) },
    { label: t("sidebar.deleteFolder"), onSelect: () => run(tree.deleteFolder(folder.id)) },
  ];

  /** The list's menu, outside any row. */
  const listMenu = (): MenuItem[] => [
    { label: t("sidebar.newSession"), onSelect: () => onNew(undefined) },
    { label: t("sidebar.newFolder"), onSelect: () => createFolder(undefined) },
  ];

  const openMenu = (e: ReactMouseEvent, items: MenuItem[]) => {
    if (followsMenuKey(e)) return;
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const fileFilters = [{ name: t("sidebar.sessionsFileType"), extensions: ["json"] }];

  const importSessions = async () => {
    const path = await openDialog({ title: t("sidebar.importSessions"), filters: fileFilters });
    if (typeof path === "string") setImporting(path);
  };

  // macOS has no shortcut for it, and marks a newer version on the settings button.
  const menuButtonLabel = isMac
    ? t("appMenu.buttonNoShortcut", { name: t("app.name") })
    : update
      ? t("appMenu.buttonUpdate", { name: t("app.name"), shortcut: shortcutLabel("appMenu"), version: update.version })
      : t("appMenu.button", { name: t("app.name"), shortcut: shortcutLabel("appMenu") });

  // From the app menu.
  useShortcuts((e) => {
    if (matches(e, "newSession")) onNew(currentFolder());
    else if (matches(e, "importSessions")) void importSessions();
    else if (matches(e, "exportSessions")) setExporting(profiles.length > 0);
    else return false;
    return true;
  });

  const importMenu = (e: ReactMouseEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: rect.left,
      y: rect.bottom + 2,
      items: [
        { label: t("sidebar.importSshConfig"), onSelect: onImportSshConfig },
        { label: t("sidebar.importSessions"), onSelect: () => void importSessions() },
        "separator",
        { label: t("sidebar.exportSessions"), disabled: profiles.length === 0, onSelect: () => setExporting(true) },
      ],
    });
  };

  // Keyboard: arrows move and fold, Enter opens a session or folds a folder.
  const onListKeyDown = (e: KeyboardEvent) => {
    if (renaming) return;
    const index = rows.findIndex((row) => rowKey(row) === selected);
    const row = rows[index];
    const select = (i: number) => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, i))];
      if (target) setSelected(rowKey(target));
    };
    if (e.key === "ArrowDown") select(index + 1);
    else if (e.key === "ArrowUp") select(index < 0 ? rows.length - 1 : index - 1);
    else if (e.key === "ArrowRight" && row?.kind === "folder" && !row.expanded) setFolderCollapsed(row.folder.id, false);
    else if (e.key === "ArrowLeft" && row?.kind === "folder" && row.expanded) setFolderCollapsed(row.folder.id, true);
    else if (e.key === "ArrowLeft" && row) {
      const parent = row.kind === "folder" ? row.folder.parent : row.profile.folder;
      if (parent) setSelected(`f:${parent}`);
    } else if (e.key === "Enter" && row?.kind === "profile") onOpen(row.profile, "connect");
    else if (e.key === "Enter" && row?.kind === "folder") setFolderCollapsed(row.folder.id, row.expanded);
    else if (isMenuKey(e)) {
      // The selected row's menu, below it; the list's own without one.
      openedByMenuKey();
      const element = listRef.current?.querySelector(`[data-key="${CSS.escape(selected ?? "")}"]`) ?? listRef.current!;
      const rect = element.getBoundingClientRect();
      const items =
        row?.kind === "profile" ? profileMenu(row.profile) : row?.kind === "folder" ? folderMenu(row.folder, row.count) : listMenu();
      setMenu({ x: rect.left + INDENT, y: rect.bottom, items });
    } else return;
    e.preventDefault();
  };

  // Keep the selected row visible as it moves.
  useEffect(() => {
    listRef.current?.querySelector(`[data-key="${CSS.escape(selected ?? "")}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const onSearchKeyDown = (e: KeyboardEvent) => {
    const pick = (index: number) => {
      if (index < results.length) onOpen(results[index], "connect");
      else if (quick) onQuickConnect(quick);
      setQuery("");
    };
    if (navigateList(e, resultCount, highlighted, setHighlight, pick) || isComposing(e)) return;
    if (e.key === "Escape") {
      if (query) setQuery("");
      else searchRef.current?.blur();
    } else return;
    e.preventDefault();
  };

  // The folder a row is in (undefined: the top level), as the tree shows it.
  // Drag and drop (see `lib/treeDrop`): over the rows, and below them for the end of the top
  // level. Not onto the Recent section, which only lists sessions.
  const dropAt = (item: TreeItem, x: number, y: number): Drop | null => {
    const element = document.elementFromPoint(x, y);
    if (!listRef.current?.contains(element)) return null;
    if (element?.closest("[data-recent], .sidebar-section")) return null;
    const rowElement = element?.closest<HTMLElement>("[data-key]");
    if (!rowElement) return { key: null };
    const rect = rowElement.getBoundingClientRect();
    return dropOnRow(item, rowElement.dataset.key!, (y - rect.top) / rect.height, rows, folders);
  };

  const applyDrop = (item: TreeItem, drop: Drop) => {
    const { parent, before } = dropMove(item, drop, state, rows);
    if (drop.key !== null && drop.position === "into" && parent) setFolderCollapsed(parent, false);
    run(tree.move(item, parent, before));
  };

  const pressDrag = usePressDrag();
  const startDrag = (e: ReactMouseEvent, item: TreeItem) => {
    if (e.button !== 0 || renaming) return;
    let current: Drop | null = null;
    let moving = false;
    pressDrag(e, {
      threshold: DRAG_THRESHOLD,
      bodyClass: "dragging-row",
      onMove: (ev) => {
        const drop = dropAt(item, ev.clientX, ev.clientY);
        // On each move of the pointer: the list renders again only for another place to drop.
        if (moving && sameDrop(drop, current)) return;
        moving = true;
        current = drop;
        setDrag({ item, drop });
      },
      onEnd: (moved) => {
        setDrag(null);
        if (moved && current) applyDrop(item, current);
      },
    });
  };

  // The row the first click of a double-click was on (null outside the rows). That click may
  // move the rows (a folder collapsing, the list scrolling back) and put another row under the
  // pointer: the browser still takes the second click there as a double-click, which mustn't
  // open that session.
  const firstClickRef = useRef<string | null>(null);
  const pressRow = (e: ReactMouseEvent, key: string) => {
    if (e.detail <= 1) firstClickRef.current = key;
  };

  const dropClass = (key: string) => {
    const drop = drag?.drop;
    return drop && drop.key === key ? ` drop-${drop.position}` : "";
  };

  // The rows' callbacks, which stay the same so that a row renders again only when what it
  // shows changes (see `SessionRow`).
  const rowActions = useStableHandlers<RowActions>({
    press: (e, key, item) => {
      pressRow(e, key);
      if (item) startDrag(e, item);
    },
    select: setSelected,
    open: (key, profile) => {
      if (firstClickRef.current === key) onOpen(profile, "connect");
    },
    profileMenu: (e, profile) => {
      setSelected(`p:${profile.id}`);
      openMenu(e, profileMenu(profile));
    },
    folderMenu: (e, folder, count) => {
      setSelected(`f:${folder.id}`);
      openMenu(e, folderMenu(folder, count));
    },
    toggle: (folder, expanded) => {
      setSelected(`f:${folder.id}`);
      setFolderCollapsed(folder.id, expanded);
    },
    finishRename,
  });

  const profileRow = (profile: Profile, key: string, depth: number, meta?: string, extra = "") => (
    <SessionRow
      key={key}
      id={rowId(key)}
      rowKey={key}
      profile={profile}
      depth={depth}
      meta={meta}
      searching={searching}
      current={searching ? extra.includes("highlighted") : selected === key}
      selected={selected === `p:${profile.id}`}
      extra={extra}
      actions={rowActions}
    />
  );

  const folderRow = (row: Extract<Row, { kind: "folder" }>) => {
    const key = `f:${row.folder.id}`;
    return (
      <FolderRow
        key={key}
        id={rowId(key)}
        folder={row.folder}
        depth={row.depth}
        expanded={row.expanded}
        count={row.count}
        selected={selected === key}
        renaming={renaming === row.folder.id}
        extra={dropClass(key)}
        actions={rowActions}
      />
    );
  };

  return (
    <aside className="sidebar" style={{ width }} ref={asideRef}>
      {/* Its part of the window's title bar, beside the traffic lights on macOS; where there are
          none, the app's icon and name take their place, as in a native title bar. */}
      <header className="sidebar-titlebar" {...DRAG_REGION}>
        {/* Opens the app menu, which macOS has in the menu bar: there it is shown only in full
            screen, where the traffic lights are hidden and the menu bar is out of sight. It
            keeps the focus where it was (a terminal), which the menu's actions act on and the
            focus goes back to. */}
        <button
          ref={appMenuRef}
          type="button"
          className="sidebar-brand app-menu-button"
          aria-label={menuButtonLabel}
          title={menuButtonLabel}
          aria-haspopup="menu"
          aria-expanded={appMenuOpen}
          onMouseDown={(e) => e.preventDefault()}
          // Opened from the keyboard (Enter, Space) when the click has no pointer position.
          onClick={(e) => toggleAppMenu(e.detail === 0)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowDown") return;
            e.preventDefault();
            if (!appMenuOpen) toggleAppMenu(true);
          }}
        >
          <img src={appIcon} alt="" />
          <span className="sidebar-brand-name">{t("app.name")}</span>
          {/* On macOS, the settings button has the mark. */}
          {update && !isMac ? <DotIcon /> : <ChevronIcon />}
        </button>
        <span className="sidebar-actions">
          <IconButton
            className={`icon-button${isMac && update ? " marked" : ""}`}
            label={
              isMac && update
                ? t("sidebar.settingsUpdate", { shortcut: shortcutLabel("settings"), version: update.version })
                : t("sidebar.settings", { shortcut: shortcutLabel("settings") })
            }
            onClick={onSettings}
          >
            <SettingsIcon />
            {isMac && update && <DotIcon size={6} />}
          </IconButton>
          <IconButton className="icon-button" label={t("sidebar.importExport")} onClick={importMenu}>
            <ImportExportIcon />
          </IconButton>
          <IconButton className="icon-button" label={t("sidebar.newSession")} onClick={() => onNew(currentFolder())}>
            <PlusIcon />
          </IconButton>
        </span>
      </header>
      <div className="sidebar-search">
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onSearchKeyDown}
          role="combobox"
          aria-label={t("sidebar.searchLabel")}
          aria-controls={listId}
          aria-expanded={searching}
          aria-autocomplete="list"
          aria-activedescendant={
            highlighted < 0 ? undefined : highlighted < results.length ? rowId(`r:${results[highlighted].id}`) : rowId("quick")
          }
          placeholder={t("sidebar.searchPlaceholder", { shortcut: shortcutLabel("searchSessions") })}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </div>
      {error && <ErrorBanner onDismiss={() => setError(null)}>{error}</ErrorBanner>}
      <div
        className={`sidebar-list${drag?.drop?.key === null ? " drop-end" : ""}`}
        ref={listRef}
        id={listId}
        // The tree keeps the focus and points screen readers at the selected row; while
        // searching, the search box does, at the highlighted result. WebKit treats a tree or
        // list with other elements than its items (and presentational ones) as a plain group:
        // the section titles are presentational and hidden from screen readers (the rows say
        // they are recent), and a list without items, which shows a message instead, has no
        // role.
        role={searching ? (results.length > 0 || quick ? "listbox" : undefined) : rows.length > 0 ? "tree" : undefined}
        aria-label={t("sidebar.listLabel")}
        aria-activedescendant={!searching && selected ? rowId(selected) : undefined}
        tabIndex={0}
        onKeyDown={onListKeyDown}
        onMouseDownCapture={(e) => {
          if (e.detail <= 1) firstClickRef.current = null;
        }}
        // Pressing outside the rows (but not on the scroll bar) clears the selection, as in
        // Finder and Explorer, so that new sessions and folders go at the top level. On the
        // press rather than the click: a click ending a drag between rows lands on the list.
        onMouseDown={(e) => {
          const list = e.currentTarget;
          const x = e.clientX - list.getBoundingClientRect().left;
          if (!(e.target as Element).closest(".tree-row") && x < list.clientLeft + list.clientWidth) setSelected(null);
        }}
        onContextMenu={(e) => openMenu(e, listMenu())}
      >
        {searching ? (
          <>
            {results.map((profile, i) =>
              profileRow(
                profile,
                `r:${profile.id}`,
                0,
                [...folderPath(profile.folder, folders), address(profile.connection)].join(" / "),
                i === highlighted ? " highlighted" : "",
              ),
            )}
            {quick && (
              <div
                id={rowId("quick")}
                role="option"
                aria-selected={highlighted === results.length}
                className={`tree-row quick${highlighted === results.length ? " highlighted" : ""}`}
                onClick={() => {
                  onQuickConnect(quick);
                  setQuery("");
                }}
              >
                <span className="tree-icon">
                  <ProtocolIcon protocol={quick.protocol} />
                </span>
                <span className="tree-name">{t("sidebar.quickConnect", { address: address(quick) })}</span>
              </div>
            )}
            {results.length === 0 && !quick && <p className="sidebar-empty">{t("sidebar.noMatches")}</p>}
          </>
        ) : (
          <>
            {recentProfiles.length > 0 && (
              <>
                <div className="sidebar-section" role="presentation" aria-hidden="true">
                  {t("sidebar.recent")}
                </div>
                {recentProfiles.map((profile) => profileRow(profile, `recent:${profile.id}`, 0))}
                <div className="sidebar-section" role="presentation" aria-hidden="true">
                  {t("sidebar.all")}
                </div>
              </>
            )}
            {rows.map((row) =>
              row.kind === "folder" ? folderRow(row) : profileRow(row.profile, rowKey(row), row.depth, undefined, dropClass(rowKey(row))),
            )}
            {/* Not before the first read: the sessions would seem gone while it comes back. */}
            {state.loaded && profiles.length === 0 && folders.length === 0 && <p className="sidebar-empty">{t("sidebar.empty")}</p>}
          </>
        )}
      </div>
      <Separator
        className="sidebar-splitter"
        label={t("sidebar.resize")}
        title={t("sidebar.resizeHint")}
        orientation="vertical"
        value={widthPercent}
        onMove={nudgeWidth}
        onReset={resetWidth}
        onMouseDown={startResize}
      />
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {exporting && <ExportDialog onClose={() => setExporting(false)} />}
      {importing && <SessionImportDialog path={importing} onClose={() => setImporting(null)} />}
      {deletingLogs &&
        (deletingLogs.count === 0 ? (
          <ConfirmDialog
            title={t("sidebar.deleteLogsTitle")}
            message={t("sidebar.noLogs", { name: deletingLogs.profile.name })}
            confirmLabel={t("common.ok")}
            onConfirm={() => setDeletingLogs(null)}
            onCancel={() => setDeletingLogs(null)}
          />
        ) : (
          <ConfirmDialog
            title={t("sidebar.deleteLogsTitle")}
            message={t("sidebar.deleteLogsMessage", { count: deletingLogs.count, name: deletingLogs.profile.name })}
            confirmLabel={t("common.delete")}
            danger
            onConfirm={() => {
              const { profile } = deletingLogs;
              setDeletingLogs(null);
              logs.delete(profile.id).catch(fail);
            }}
            onCancel={() => setDeletingLogs(null)}
          />
        ))}
    </aside>
  );
}
