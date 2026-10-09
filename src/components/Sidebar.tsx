import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

import {
  deleteProfile,
  duplicateProfile,
  errorMessage,
  logs,
  sessionsFile,
  tree,
  type Folder,
  type Profile,
  type TreeItem,
} from "../lib/api";
import { dragHorizontally } from "../lib/drag";
import { searchShortcutLabel, settingsShortcutLabel } from "../lib/platform";
import {
  RECENT_LIMIT,
  address,
  folderPath,
  isWithin,
  parseQuickConnect,
  rowKey,
  search,
  storeCollapsed,
  storedCollapsed,
  treeRows,
  type QuickTarget,
  type Row,
} from "../lib/sessions";
import { DRAG_REGION } from "../lib/window";
import appIcon from "../../src-tauri/icons/source/icon.svg";
import { PlusIcon } from "./icons";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { SessionImportDialog } from "./SessionImportDialog";

/**
 * How a session is opened: switch to a pane it already has, always in a new tab, or in a
 * new pane right of or below the focused one.
 */
export type OpenMode = "connect" | "newTab" | "splitRight" | "splitDown";

interface Props {
  profiles: Profile[];
  folders: Folder[];
  /** Recently opened session ids, most recent first; empty hides the section. */
  recent: string[];
  /** Changes when the search shortcut is pressed, to focus the search box. */
  searchFocusKey: number;
  onOpen(profile: Profile, mode: OpenMode): void;
  onOpenAll(folder: Folder): void;
  onQuickConnect(target: QuickTarget): void;
  onEdit(profile: Profile): void;
  /** Creates a session in `folder` (undefined: top level). */
  onNew(folder: string | undefined): void;
  /** Sessions or folders changed here; reload them. */
  onChanged(): void;
  onImportSshConfig(): void;
  onSettings(): void;
}

const DEFAULT_WIDTH = 220;
const MIN_WIDTH = 160;
const MAX_WIDTH = 480;
/** Space always left for the terminal area. */
const MIN_MAIN = 400;
const WIDTH_KEY = "zshell.sidebarWidth";
/** Indentation per folder level, in pixels. */
const INDENT = 14;
/** How far the pointer moves before a press on a row becomes a drag. */
const DRAG_THRESHOLD = 4;

/** The width saved by the last resize; layout state, so kept per machine rather than in the settings. */
function storedWidth(): number {
  try {
    const width = Number(localStorage.getItem(WIDTH_KEY));
    return width >= MIN_WIDTH && width <= MAX_WIDTH ? width : DEFAULT_WIDTH;
  } catch {
    return DEFAULT_WIDTH;
  }
}

function storeWidth(width: number) {
  try {
    localStorage.setItem(WIDTH_KEY, String(width));
  } catch {
    // Not persisted; the width still applies until the app restarts.
  }
}

/** Where a dragged row would land: relative to a row, or at the end of the top level. */
type Drop = { key: string; position: "before" | "after" | "into" } | { key: null };

export function Sidebar(props: Props) {
  const { profiles, folders, recent, searchFocusKey, onOpen, onOpenAll, onQuickConnect, onEdit, onNew, onChanged } = props;
  const { t } = useTranslation();
  const [width, setWidth] = useState(storedWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const asideRef = useRef<HTMLElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  // Highlighted search result (index into results, then the quick connect line).
  const [highlight, setHighlight] = useState(0);
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

  const rows = treeRows(folders, profiles, collapsed);
  const searching = query.trim() !== "";
  const results = searching ? search(query, folders, profiles) : [];
  const quick = searching && results.length === 0 ? parseQuickConnect(query) : null;
  const recentProfiles = recent
    .map((id) => profiles.find((p) => p.id === id))
    .filter((p): p is Profile => !!p)
    .slice(0, RECENT_LIMIT);

  useEffect(() => {
    if (searchFocusKey === 0) return;
    searchRef.current?.focus();
    searchRef.current?.select();
  }, [searchFocusKey]);

  useEffect(() => setHighlight(0), [query]);

  const fail = (e: unknown) => setError(errorMessage(e));
  const run = (action: Promise<unknown>) => void action.then(onChanged, fail);

  const setFolderCollapsed = (id: string, value: boolean) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (value) next.add(id);
      else next.delete(id);
      storeCollapsed(next);
      return next;
    });

  const startResize = (e: ReactMouseEvent) => {
    const left = asideRef.current!.getBoundingClientRect().left;
    dragHorizontally(
      e,
      (x) => setWidth(Math.round(Math.max(MIN_WIDTH, Math.min(x - left, MAX_WIDTH, window.innerWidth - MIN_MAIN)))),
      () => storeWidth(widthRef.current),
    );
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
      onChanged();
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
    duplicateProfile(profile.id, t("sidebar.copyName", { name: profile.name })).then((copy) => {
      onChanged();
      setSelected(`p:${copy.id}`);
    }, fail);
  };

  const profileMenu = (profile: Profile): MenuItem[] => [
    { label: t("sidebar.connect"), onSelect: () => onOpen(profile, "connect") },
    { label: t("sidebar.connectNewTab"), onSelect: () => onOpen(profile, "newTab") },
    { label: t("sidebar.connectSplitRight"), onSelect: () => onOpen(profile, "splitRight") },
    { label: t("sidebar.connectSplitDown"), onSelect: () => onOpen(profile, "splitDown") },
    "separator",
    { label: t("sidebar.edit"), onSelect: () => onEdit(profile) },
    { label: t("sidebar.duplicate"), onSelect: () => duplicate(profile) },
    { label: t("sidebar.copyAddress"), onSelect: () => void writeText(address(profile)).catch(fail) },
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

  const openMenu = (e: ReactMouseEvent, items: MenuItem[]) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const fileFilters = [{ name: t("sidebar.sessionsFileType"), extensions: ["json"] }];

  const exportSessions = async () => {
    const path = await saveDialog({ title: t("sidebar.exportSessions"), defaultPath: "zshell-sessions.json", filters: fileFilters });
    if (path) sessionsFile.export(path).catch(fail);
  };

  const importSessions = async () => {
    const path = await openDialog({ title: t("sidebar.importSessions"), filters: fileFilters });
    if (typeof path === "string") setImporting(path);
  };

  const importMenu = (e: ReactMouseEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: rect.left,
      y: rect.bottom + 2,
      items: [
        { label: t("sidebar.importSshConfig"), onSelect: props.onImportSshConfig },
        { label: t("sidebar.importSessions"), onSelect: () => void importSessions() },
        "separator",
        { label: t("sidebar.exportSessions"), disabled: profiles.length === 0, onSelect: () => void exportSessions() },
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
    } else if (e.key === "Enter" && row?.kind === "profile") onOpen(row.profile, "newTab");
    else if (e.key === "Enter" && row?.kind === "folder") setFolderCollapsed(row.folder.id, row.expanded);
    else return;
    e.preventDefault();
  };

  // Keep the selected row visible as it moves.
  useEffect(() => {
    listRef.current?.querySelector(`[data-key="${CSS.escape(selected ?? "")}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const onSearchKeyDown = (e: KeyboardEvent) => {
    const count = results.length + (quick ? 1 : 0);
    if (e.key === "ArrowDown") setHighlight((h) => Math.min(h + 1, count - 1));
    else if (e.key === "ArrowUp") setHighlight((h) => Math.max(h - 1, 0));
    else if (e.key === "Enter") {
      if (highlight < results.length) onOpen(results[highlight], "newTab");
      else if (quick) onQuickConnect(quick);
      else return;
      setQuery("");
    } else if (e.key === "Escape") {
      if (query) setQuery("");
      else searchRef.current?.blur();
    } else return;
    e.preventDefault();
  };

  // Drag and drop: rows move before or after rows of their kind, sessions and folders into
  // folders, and anything to the end of the top level below the last row.
  const dropAt = (item: TreeItem, x: number, y: number): Drop | null => {
    const element = document.elementFromPoint(x, y);
    if (!listRef.current?.contains(element)) return null;
    const rowElement = element?.closest<HTMLElement>("[data-key]");
    if (!rowElement) return { key: null };
    const key = rowElement.dataset.key!;
    if (key === `${item.kind === "folder" ? "f" : "p"}:${item.id}`) return null;
    const rect = rowElement.getBoundingClientRect();
    const offset = (y - rect.top) / rect.height;
    if (key.startsWith("f:")) {
      const target = key.slice(2);
      if (item.kind === "folder" && isWithin(target, item.id, folders)) return null;
      if (item.kind === "profile") return { key, position: "into" };
      return { key, position: offset < 0.25 ? "before" : offset > 0.75 ? "after" : "into" };
    }
    if (item.kind === "folder") return null;
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
    // Before or after a row of the same kind: in its folder, before it or its next sibling.
    const siblings: { id: string; parent?: string }[] =
      item.kind === "folder" ? folders : profiles.map((p) => ({ id: p.id, parent: p.folder }));
    const target = siblings.find((s) => s.id === id)!;
    const peers = siblings.filter((s) => s.parent === target.parent && s.id !== item.id);
    const at = peers.findIndex((s) => s.id === id) + (drop.position === "after" ? 1 : 0);
    run(tree.move(item, target.parent ?? null, peers[at]?.id ?? null));
  };

  const startDrag = (e: ReactMouseEvent, item: TreeItem) => {
    if (e.button !== 0 || renaming) return;
    const startX = e.clientX;
    const startY = e.clientY;
    let current: Drop | null = null;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD) return;
      moved = true;
      current = dropAt(item, ev.clientX, ev.clientY);
      setDrag({ item, drop: current });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("dragging-row");
      setDrag(null);
      if (moved && current) applyDrop(item, current);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    document.body.classList.add("dragging-row");
  };

  const dropClass = (key: string) => {
    const drop = drag?.drop;
    return drop && drop.key === key ? ` drop-${drop.position}` : "";
  };

  const profileRow = (profile: Profile, key: string, depth: number, meta?: string, extra = "") => (
    <div
      key={key}
      data-key={key.startsWith("p:") ? key : undefined}
      className={`tree-row session${selected === `p:${profile.id}` ? " selected" : ""}${extra}`}
      style={{ paddingLeft: 8 + depth * INDENT + 16 }}
      onMouseDown={(e) => key.startsWith("p:") && startDrag(e, { kind: "profile", id: profile.id })}
      onClick={() => setSelected(`p:${profile.id}`)}
      onDoubleClick={() => onOpen(profile, "newTab")}
      onContextMenu={(e) => {
        setSelected(`p:${profile.id}`);
        openMenu(e, profileMenu(profile));
      }}
      title={`${profile.name}\n${address(profile)}\n${t("sidebar.openHint")}`}
    >
      <span className="tree-name">{profile.name}</span>
      <span className="tree-meta">{meta ?? address(profile)}</span>
    </div>
  );

  const folderRow = (row: Extract<Row, { kind: "folder" }>) => {
    const { folder, depth, expanded, count } = row;
    const key = `f:${folder.id}`;
    return (
      <div
        key={key}
        data-key={key}
        className={`tree-row folder${selected === key ? " selected" : ""}${dropClass(key)}`}
        style={{ paddingLeft: 8 + depth * INDENT }}
        onMouseDown={(e) => startDrag(e, { kind: "folder", id: folder.id })}
        onClick={() => {
          setSelected(key);
          setFolderCollapsed(folder.id, expanded);
        }}
        onDoubleClick={() => count > 0 && onOpenAll(folder)}
        onContextMenu={(e) => {
          setSelected(key);
          openMenu(e, folderMenu(folder, count));
        }}
        title={t("sidebar.folderHint")}
      >
        <Chevron open={expanded} />
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
        <span className="sidebar-brand" aria-hidden="true">
          <img src={appIcon} alt="" />
          <span className="sidebar-brand-name">{t("app.name")}</span>
        </span>
        <span className="sidebar-actions">
          <button className="icon-button" title={t("sidebar.importExport")} onClick={importMenu}>
            <ImportExportIcon />
          </button>
          <button className="icon-button" title={t("sidebar.newSession")} onClick={() => onNew(currentFolder())}>
            <PlusIcon />
          </button>
        </span>
      </header>
      <div className="sidebar-search">
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onSearchKeyDown}
          placeholder={t("sidebar.searchPlaceholder", { shortcut: searchShortcutLabel })}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </div>
      {error && (
        <div className="panel-error" onClick={() => setError(null)} title={t("sftp.dismissHint")}>
          {error}
        </div>
      )}
      <div
        className={`sidebar-list${drag?.drop?.key === null ? " drop-end" : ""}`}
        ref={listRef}
        tabIndex={0}
        onKeyDown={onListKeyDown}
        onContextMenu={(e) =>
          openMenu(e, [
            { label: t("sidebar.newSession"), onSelect: () => onNew(undefined) },
            { label: t("sidebar.newFolder"), onSelect: () => createFolder(undefined) },
          ])
        }
      >
        {searching ? (
          <>
            {results.map((profile, i) =>
              profileRow(
                profile,
                `r:${profile.id}`,
                -1,
                [...folderPath(profile.folder, folders), address(profile)].join(" / "),
                i === highlight ? " highlighted" : "",
              ),
            )}
            {quick && (
              <div
                className={`tree-row quick${highlight === results.length ? " highlighted" : ""}`}
                onClick={() => {
                  onQuickConnect(quick);
                  setQuery("");
                }}
              >
                <span className="tree-name">{t("sidebar.quickConnect", { address: address(quick) })}</span>
              </div>
            )}
            {results.length === 0 && !quick && <p className="sidebar-empty">{t("sidebar.noMatches")}</p>}
          </>
        ) : (
          <>
            {recentProfiles.length > 0 && (
              <>
                <div className="sidebar-section">{t("sidebar.recent")}</div>
                {recentProfiles.map((profile) => profileRow(profile, `recent:${profile.id}`, -1))}
                <div className="sidebar-section">{t("sidebar.all")}</div>
              </>
            )}
            {rows.map((row) =>
              row.kind === "folder"
                ? folderRow(row)
                : profileRow(row.profile, rowKey(row), row.depth, undefined, dropClass(rowKey(row))),
            )}
            {profiles.length === 0 && folders.length === 0 && <p className="sidebar-empty">{t("sidebar.empty")}</p>}
          </>
        )}
      </div>
      <footer className="sidebar-footer">
        <button className="sidebar-settings" onClick={props.onSettings}>
          <SettingsIcon />
          <span>{t("sidebar.settings")}</span>
          <kbd>{settingsShortcutLabel}</kbd>
        </button>
      </footer>
      <div className="sidebar-splitter" onMouseDown={startResize} onDoubleClick={resetWidth} title={t("sidebar.resizeHint")} />
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {importing && <SessionImportDialog path={importing} onClose={() => setImporting(null)} onImported={onChanged} />}
      {deleting && (
        <ConfirmDialog
          title={t("sidebar.deleteTitle")}
          message={t("sidebar.deleteMessage", { name: deleting.name })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => {
            const profile = deleting;
            setDeleting(null);
            run(deleteProfile(profile.id));
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
        if (e.key === "Enter") finish(e.currentTarget.value.trim() || null);
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={(e) => finish(e.target.value.trim() || null)}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
    />
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg className={`chevron${open ? " open" : ""}`} width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M3.5 2l3 3-3 3" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** Arrows up and down, for the import and export menu. */
function ImportExportIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 13V3M2.5 5.5L5 3l2.5 2.5M11 3v10M8.5 10.5L11 13l2.5-2.5" />
    </svg>
  );
}

/** Sliders, drawn as an SVG rather than a text glyph so it centers the same in every font. */
function SettingsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true">
      <path d="M2 4h1.4M6.6 4H14M2 8h7.4M12.6 8H14M2 12h3.4M8.6 12H14" />
      <circle cx="5" cy="4" r="1.6" />
      <circle cx="11" cy="8" r="1.6" />
      <circle cx="7" cy="12" r="1.6" />
    </svg>
  );
}
