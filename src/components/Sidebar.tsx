import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import type { AppMenuButton } from "../app/useAppMenu";
import { announce } from "../lib/announce";
import { deleteProfile, duplicateProfile, errorMessage, logs, tree, type Folder, type Profile, type TreeItem } from "../ipc/api";
import { directory, recentProfiles as recentOf } from "../lib/directory";
import { useDirectory } from "../hooks/stores";
import { dragHorizontally, usePressDrag } from "../hooks/drag";
import { open as openDialog } from "../ipc/fileDialog";
import { clampIndex, navigateList } from "../lib/listNavigation";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import { matches } from "../lib/keymap";
import { isComposing, isMac } from "../lib/platform";
import {
  address,
  childItem,
  childrenByFolder,
  folderPath,
  isWithin,
  itemKey,
  notesPreview,
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
import { storedString, storeString } from "../lib/storage";
import { DRAG_REGION } from "../hooks/titleBar";
import appIcon from "../../src-tauri/icons/source/icon.svg";
import { useLatest } from "../hooks/latest";
import { useLocale } from "../hooks/locale";
import { ChevronIcon, DisclosureIcon, DotIcon, ImportExportIcon, PlusIcon, ProtocolIcon, SettingsIcon } from "./icons";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { ErrorBanner } from "./ErrorMessage";
import { IconButton } from "./IconButton";
import { Separator } from "./Separator";
import { ExportDialog } from "./ExportDialog";
import { SessionImportDialog } from "./SessionImportDialog";

/**
 * How a session is opened: switch to a pane it already has, always in a new tab, or in a
 * new pane right of or below the focused one.
 */
export type OpenMode = "connect" | "newTab" | "splitRight" | "splitDown" | "newWindow";

interface Props {
  /** Changes when the search shortcut is pressed, to focus the search box. */
  searchFocusKey: number;
  onOpen(profile: Profile, mode: OpenMode): void;
  onOpenAll(folder: Folder): void;
  onQuickConnect(target: QuickTarget): void;
  onEdit(profile: Profile): void;
  /** Creates a session in `folder` (undefined: top level). */
  onNew(folder: string | undefined): void;
  onImportSshConfig(): void;
  onSettings(): void;
  /**
   * The app menu, opened by the app's icon and name outside macOS; a newer version marks it,
   * and on macOS the settings button, where the check is.
   */
  appMenu: AppMenuButton;
}

const DEFAULT_WIDTH = 260;
// Room for the traffic lights and the three buttons of the title bar.
const MIN_WIDTH = 176;
const MAX_WIDTH = 480;
/** Space always left for the terminal area. */
const MIN_MAIN = 400;
const WIDTH_KEY = "zshell.sidebarWidth";
/**
 * Indentation per folder level, in pixels: a row's icon (a folder's disclosure triangle, a
 * session's protocol) and the gap after it, so that the rows in a folder start below its name.
 */
const INDENT = 20;
/** How far the pointer moves before a press on a row becomes a drag. */
const DRAG_THRESHOLD = 4;

/** The width saved by the last resize; layout state, so kept per machine rather than in the settings. */
function storedWidth(): number {
  const width = Number(storedString(WIDTH_KEY));
  // One saved under other limits (the minimum used to be smaller) is brought within these.
  return width > 0 ? Math.min(Math.max(width, MIN_WIDTH), MAX_WIDTH) : DEFAULT_WIDTH;
}

const storeWidth = (width: number) => storeString(WIDTH_KEY, String(width));

/** Where a dragged row would land: relative to a row, or at the end of the top level. */
type Drop = { key: string; position: "before" | "after" | "into" } | { key: null };

export function Sidebar(props: Props) {
  const { searchFocusKey, onOpen, onOpenAll, onQuickConnect, onEdit, onNew } = props;
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const { settings } = useSettings();
  const state = useDirectory();
  const { profiles, folders } = state;
  // Taken apart here: React Compiler treats an object holding a ref as a ref, and reading any of
  // it while rendering would keep the whole component from being compiled.
  const { ref: appMenuRef, open: appMenuOpen, update, toggle: toggleAppMenu } = props.appMenu;
  const [width, setWidth] = useState(storedWidth);
  const widthRef = useLatest(width);
  const asideRef = useRef<HTMLElement>(null);
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
  const [deleting, setDeleting] = useState<Profile | null>(null);
  // A session whose logs are about to be deleted, with how many it has.
  const [deletingLogs, setDeletingLogs] = useState<{ profile: Profile; count: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ item: TreeItem; drop: Drop | null } | null>(null);
  // A sessions file being imported.
  const [importing, setImporting] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  const rows = treeRows(state, collapsed);
  const searching = query.trim() !== "";
  const results = searching ? search(query, state) : [];
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
  const run = (action: Promise<unknown>) => void action.then(directory.reload, fail);

  const setFolderCollapsed = (id: string, value: boolean) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (value) next.add(id);
      else next.delete(id);
      storeCollapsed(next);
      return next;
    });

  const clampWidth = (width: number) => Math.round(Math.max(MIN_WIDTH, Math.min(width, MAX_WIDTH, window.innerWidth - MIN_MAIN)));

  const startResize = (e: ReactMouseEvent) => {
    const left = asideRef.current!.getBoundingClientRect().left;
    dragHorizontally(
      e,
      (x) => setWidth(clampWidth(x - left)),
      () => storeWidth(widthRef.current),
    );
  };

  const nudgeWidth = (pixels: number) => {
    const next = clampWidth(widthRef.current + pixels);
    setWidth(next);
    storeWidth(next);
  };

  const resetWidth = () => {
    setWidth(DEFAULT_WIDTH);
    storeWidth(DEFAULT_WIDTH);
  };

  /** The folder new things go in: the selected folder, or the selected session's. */
  const currentFolder = () => {
    if (selected?.startsWith("f:")) return selected.slice(2);
    if (selected?.startsWith("p:")) return profiles.find((p) => p.id === selected.slice(2))?.folder;
    return undefined;
  };

  const createFolder = (parent: string | undefined) => {
    tree.saveFolder({ id: "", name: t("sidebar.newFolderName"), parent }).then((folder) => {
      if (parent) setFolderCollapsed(parent, false);
      void directory.reload();
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
    duplicateProfile(profile.id, t("sidebar.copyName", { name: profile.name })).then((saved) => {
      void directory.reload();
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
    { label: t("sidebar.delete"), danger: true, onSelect: () => setDeleting(profile) },
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
        { label: t("sidebar.importSshConfig"), onSelect: props.onImportSshConfig },
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
  const parentOf = (key: string) => {
    const row = rows.find((row) => rowKey(row) === key);
    if (row?.kind === "folder") return row.folder.parent;
    const folder = row?.profile.folder;
    return folder !== undefined && folders.some((f) => f.id === folder) ? folder : undefined;
  };

  // Drag and drop: rows move before or after other rows, sessions and folders mixed, into
  // folders, and to the end of the top level below the last row. Not onto the Recent
  // section, which only lists sessions. A folder can't go into itself or its subfolders.
  const dropAt = (item: TreeItem, x: number, y: number): Drop | null => {
    const element = document.elementFromPoint(x, y);
    if (!listRef.current?.contains(element)) return null;
    if (element?.closest("[data-recent], .sidebar-section")) return null;
    const rowElement = element?.closest<HTMLElement>("[data-key]");
    if (!rowElement) return { key: null };
    const key = rowElement.dataset.key!;
    if (key === itemKey(item)) return null;
    if (item.kind === "folder" && isWithin(parentOf(key), item.id, folders)) return null;
    const rect = rowElement.getBoundingClientRect();
    const offset = (y - rect.top) / rect.height;
    if (key.startsWith("f:")) {
      const target = key.slice(2);
      if (item.kind === "folder" && isWithin(target, item.id, folders)) return null;
      // Right below an expanded folder are its own rows: "after" it means after all of them,
      // which a line there would not show.
      const index = rows.findIndex((row) => row.kind === "folder" && row.folder.id === target);
      const open = index >= 0 && (rows[index + 1]?.depth ?? 0) > rows[index].depth;
      return { key, position: offset < 0.25 ? "before" : offset > 0.75 && !open ? "after" : "into" };
    }
    return { key, position: offset < 0.5 ? "before" : "after" };
  };

  const applyDrop = (item: TreeItem, drop: Drop) => {
    if (drop.key === null) {
      run(tree.move(item, null, null));
      return;
    }
    const id = drop.key.slice(2);
    if (drop.position === "into") {
      setFolderCollapsed(id, false);
      run(tree.move(item, id, null));
      return;
    }
    // Before or after a row: in its folder, before it or the row that follows it there.
    const parent = parentOf(drop.key);
    const peers = (childrenByFolder(state).get(parent) ?? []).map(childItem).filter((peer) => itemKey(peer) !== itemKey(item));
    const at = peers.findIndex((peer) => itemKey(peer) === drop.key) + (drop.position === "after" ? 1 : 0);
    run(tree.move(item, parent ?? null, peers[at] ?? null));
  };

  const pressDrag = usePressDrag();
  const startDrag = (e: ReactMouseEvent, item: TreeItem) => {
    if (e.button !== 0 || renaming) return;
    let current: Drop | null = null;
    pressDrag(e, {
      threshold: DRAG_THRESHOLD,
      bodyClass: "dragging-row",
      onMove: (ev) => {
        current = dropAt(item, ev.clientX, ev.clientY);
        setDrag({ item, drop: current });
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

  const profileRow = (profile: Profile, key: string, depth: number, meta?: string, extra = "") => (
    <div
      key={key}
      id={rowId(key)}
      role={searching ? "option" : "treeitem"}
      aria-selected={searching ? extra.includes("highlighted") : selected === key}
      aria-level={searching ? undefined : depth + 1}
      // Rather than the tooltip, which is also read, as a hint.
      aria-label={t(key.startsWith("recent:") ? "sidebar.recentRowLabel" : "sidebar.rowLabel", {
        name: profile.name,
        address: meta ?? address(profile.connection),
      })}
      data-key={key.startsWith("p:") ? key : undefined}
      data-recent={key.startsWith("recent:") || undefined}
      className={`tree-row session${selected === `p:${profile.id}` ? " selected" : ""}${extra}`}
      style={{ paddingLeft: 8 + depth * INDENT }}
      onMouseDown={(e) => {
        pressRow(e, key);
        if (key.startsWith("p:")) startDrag(e, { kind: "profile", id: profile.id });
      }}
      onClick={() => setSelected(`p:${profile.id}`)}
      onDoubleClick={() => firstClickRef.current === key && onOpen(profile, "connect")}
      onContextMenu={(e) => {
        setSelected(`p:${profile.id}`);
        openMenu(e, profileMenu(profile));
      }}
      title={[profile.name, address(profile.connection), profile.notes && `\n${notesPreview(profile.notes)}\n`, t("sidebar.openHint")]
        .filter(Boolean)
        .join("\n")}
    >
      <span className="tree-icon">
        <ProtocolIcon protocol={profile.connection.protocol} />
      </span>
      <span className="tree-name">{profile.name}</span>
      <span className="tree-meta">{meta ?? address(profile.connection)}</span>
    </div>
  );

  const folderRow = (row: Extract<Row, { kind: "folder" }>) => {
    const { folder, depth, expanded, count } = row;
    const key = `f:${folder.id}`;
    return (
      <div
        key={key}
        id={rowId(key)}
        role="treeitem"
        aria-selected={selected === key}
        aria-expanded={expanded}
        aria-level={depth + 1}
        aria-label={t("sidebar.folderLabel", { name: folder.name, count })}
        data-key={key}
        className={`tree-row folder${selected === key ? " selected" : ""}${dropClass(key)}`}
        style={{ paddingLeft: 8 + depth * INDENT }}
        onMouseDown={(e) => {
          pressRow(e, key);
          startDrag(e, { kind: "folder", id: folder.id });
        }}
        onClick={() => {
          setSelected(key);
          setFolderCollapsed(folder.id, expanded);
        }}
        onContextMenu={(e) => {
          setSelected(key);
          openMenu(e, folderMenu(folder, count));
        }}
      >
        <span className="tree-icon">
          <DisclosureIcon open={expanded} />
        </span>
        {renaming === folder.id ? (
          <NameEditor initial={folder.name} onDone={(name) => finishRename(folder, name)} />
        ) : (
          <span className="tree-name">{folder.name}</span>
        )}
        <span className="tree-count">{count}</span>
      </div>
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
            onClick={props.onSettings}
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
            {profiles.length === 0 && folders.length === 0 && <p className="sidebar-empty">{t("sidebar.empty")}</p>}
          </>
        )}
      </div>
      <Separator
        className="sidebar-splitter"
        label={t("sidebar.resize")}
        title={t("sidebar.resizeHint")}
        orientation="vertical"
        value={((width - MIN_WIDTH) / (MAX_WIDTH - MIN_WIDTH)) * 100}
        onMove={nudgeWidth}
        onReset={resetWidth}
        onMouseDown={startResize}
      />
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {exporting && <ExportDialog onClose={() => setExporting(false)} />}
      {importing && <SessionImportDialog path={importing} onClose={() => setImporting(null)} />}
      {deleting && (
        <ConfirmDialog
          title={t("sidebar.deleteTitle")}
          message={t("sidebar.deleteMessage", { name: deleting.name })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => {
            const profile = deleting;
            setDeleting(null);
            deleteProfile(profile.id).then(() => void directory.reload(), fail);
          }}
          onCancel={() => setDeleting(null)}
        />
      )}
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

/** Inline editor for a folder name: Enter or leaving keeps it, Escape cancels (`null`). */
function NameEditor({ initial, onDone }: { initial: string; onDone(name: string | null): void }) {
  const done = useRef(false);
  const finish = (name: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(name);
  };
  return (
    <input
      className="tree-name-input"
      defaultValue={initial}
      autoFocus
      onFocus={(e) => e.target.select()}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (isComposing(e)) return;
        if (e.key === "Enter") finish(e.currentTarget.value.trim() || null);
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish(e.target.value.trim() || null)}
      autoCapitalize="off"
      autoCorrect="off"
    />
  );
}
