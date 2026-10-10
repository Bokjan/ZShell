import { useCallback, useEffect, useId, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { dirname, join } from "@tauri-apps/api/path";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";

import { announce } from "../lib/announce";
import { errorCode, errorMessage, sftp, type FileEntry, type SessionId } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { pathRange, storeView, storedView, visibleEntries, type FileView, type SortKey } from "../lib/fileList";
import { basename, formatMode, formatSize, formatTime } from "../lib/format";
import { isComposing, isMac } from "../lib/platform";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { IconButton } from "./IconButton";
import { ArrowIcon, ChevronIcon, CloseIcon, EyeIcon, RefreshIcon, SearchIcon } from "./icons";
import { Modal } from "./Modal";
import { TransferList, type Transfer } from "./TransferList";

interface Props {
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether this panel is visible and should receive file drops. */
  active: boolean;
  /** How many uploads and downloads are running, each time that changes. */
  onTransfers(count: number): void;
}

interface Confirm {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  action(): void;
  /** A later question with the same key replaces this one, if it is still waiting. */
  key?: string;
}

interface Menu {
  x: number;
  y: number;
  items: MenuItem[];
}

/** A mouse press on a row that becomes a drag once the mouse moves far enough. */
interface Press {
  x: number;
  y: number;
  path: string;
}

const DRAG_DISTANCE = 5;
const DOWNLOAD_TO_KEY = "zshell.downloadToDirectory";

const joinPath = (dir: string, name: string) => (dir.endsWith("/") ? dir + name : `${dir}/${name}`);
const parentPath = (path: string) => path.replace(/\/[^/]+\/?$/, "") || "/";

/** Whether moving `paths` into `dir` would do nothing or put a folder inside itself. */
const isPointlessMove = (paths: string[], dir: string) =>
  paths.some((path) => dir === path || dir.startsWith(`${path}/`)) || paths.every((path) => parentPath(path) === dir);

function storedDownloadTo() {
  try {
    return localStorage.getItem(DOWNLOAD_TO_KEY);
  } catch {
    return null;
  }
}

function storeDownloadTo(dir: string) {
  try {
    localStorage.setItem(DOWNLOAD_TO_KEY, dir);
  } catch {
    // The dialog then opens in its default place.
  }
}

/** The permissions dialog's backdrop, which makes it a dialog (see `useDialog`). */
function ChmodDialog({ onClose, children }: { onClose(): void; children: ReactNode }) {
  const dialog = useDialog(onClose);
  return <Modal dialog={dialog}>{children}</Modal>;
}

export function SftpPanel({ sessionId, connected, active, onTransfers }: Props) {
  const { t } = useTranslation();
  const [cwd, setCwd] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<FileView>(storedView);
  /** The name filter; null while the filter field is closed. */
  const [filter, setFilter] = useState<string | null>(null);
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  /** Where a ⇧ range starts, and the row the arrow keys move from. */
  const [anchor, setAnchor] = useState<string | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const gridId = useId();
  const [renaming, setRenaming] = useState<{ path: string; value: string } | null>(null);
  const [newFolder, setNewFolder] = useState<string | null>(null);
  const [chmodTarget, setChmodTarget] = useState<{ entry: FileEntry; value: string } | null>(null);
  // Shown one at a time: an edit conflict found by a background save waits behind a delete
  // or replace the user is answering, rather than taking its place.
  const [confirms, setConfirms] = useState<(Confirm & { id: number })[]>([]);
  const confirm = confirms[0] ?? null;
  const confirmIds = useRef(0);
  const ask = (question: Confirm) => {
    const asked = { ...question, id: ++confirmIds.current };
    setConfirms((queue) => {
      const at = question.key == null ? -1 : queue.findIndex((q, i) => i > 0 && q.key === question.key);
      return at < 0 ? [...queue, asked] : queue.map((q, i) => (i === at ? asked : q));
    });
  };
  const answered = () => setConfirms((queue) => queue.slice(1));
  const [menu, setMenu] = useState<Menu | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  /** Files from another application are over the panel. */
  const [dragOver, setDragOver] = useState(false);
  /** The folder that what is being dragged would go into. */
  const [dropDir, setDropDir] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const cwdRef = useRef<string | null>(null);
  cwdRef.current = cwd;
  const sessionRef = useRef({ sessionId, connected });
  sessionRef.current = { sessionId, connected };
  const transfersRef = useRef(transfers);
  transfersRef.current = transfers;
  const pressRef = useRef<Press | null>(null);
  /** The rows the last two presses in the list were on (null elsewhere). */
  const pressedRowsRef = useRef<[string | null, string | null]>([null, null]);
  /** The paths being dragged out of this panel, until shortly after the drag ends. */
  const draggingRef = useRef<string[] | null>(null);
  const dropDirRef = useRef<string | null>(null);

  const visible = useMemo(() => visibleEntries(entries, view, filter ?? ""), [entries, view, filter]);
  // What actions apply to: the selected entries that are shown, in the listed order.
  const selected = useMemo(() => visible.filter((entry) => selection.has(entry.path)), [visible, selection]);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const entriesRef = useRef(entries);
  entriesRef.current = entries;

  const fail = (e: unknown) => setError(errorMessage(e));

  const changeView = (patch: Partial<FileView>) =>
    setView((current) => {
      const next = { ...current, ...patch };
      storeView(next);
      return next;
    });

  const select = (paths: string[], anchorPath: string | null = paths[0] ?? null) => {
    setSelection(new Set(paths));
    setAnchor(anchorPath);
    setCursor(paths.length > 0 ? paths[paths.length - 1] : null);
  };

  /** Counts listings asked for: only the latest one's answer is shown. */
  const loadSeqRef = useRef(0);
  const load = useCallback(
    async (path: string) => {
      if (sessionId == null) return;
      const seq = ++loadSeqRef.current;
      // A slower answer for a folder opened before, or from the connection before a
      // reconnection, must not replace what came after it.
      const stale = () => seq !== loadSeqRef.current || sessionRef.current.sessionId !== sessionId;
      setLoading(true);
      try {
        const listing = await sftp.list(sessionId, path);
        if (stale()) return;
        // Refreshing keeps the selection; another folder starts afresh.
        if (listing.path !== cwdRef.current) {
          setSelection(new Set());
          setAnchor(null);
          setCursor(null);
          setFilter(null);
        }
        setCwd(listing.path);
        setPathInput(listing.path);
        setEntries(listing.entries);
        setError(null);
      } catch (e) {
        if (stale()) return;
        setPathInput(cwdRef.current ?? "");
        fail(e);
      } finally {
        if (seq === loadSeqRef.current) setLoading(false);
      }
    },
    [sessionId],
  );

  // (Re)open on connect, staying in the current directory across reconnects.
  useEffect(() => {
    if (sessionId == null || !connected) return;
    if (cwdRef.current) void load(cwdRef.current);
    else sftp.open(sessionId).then(load, fail);
  }, [sessionId, connected, load]);

  const refresh = () => cwd && load(cwd);

  // Finished and failed transfers are announced to screen readers: the list may be out of
  // sight, or the panel closed.
  const transferStatuses = useRef(new Map<string, Transfer["status"]>());
  useEffect(() => {
    for (const transfer of transfers) {
      if (transferStatuses.current.get(transfer.id) === transfer.status) continue;
      transferStatuses.current.set(transfer.id, transfer.status);
      if (transfer.status === "done") announce(t("announce.transferDone", { name: transfer.label }));
      else if (transfer.status === "error") {
        announce(t("announce.transferFailed", { name: transfer.label, error: transfer.error ?? "" }));
      }
    }
  }, [transfers, t]);

  const updateTransfer = (id: string, patch: Partial<Transfer>) =>
    setTransfers((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));

  const failTransfer = (id: string, e: unknown) =>
    updateTransfer(
      id,
      errorCode(e) === "transfer.cancelled" ? { status: "cancelled" } : { status: "error", error: errorMessage(e) },
    );

  const labelFor = (names: string[]) =>
    names.length === 1 ? names[0] : t("transfer.labelMore", { name: names[0], count: names.length - 1 });

  const runTransfer = async (kind: Transfer["kind"], names: string[], run: (id: string) => Promise<string[] | void>) => {
    const id = crypto.randomUUID();
    setTransfers((ts) => [{ id, kind, label: labelFor(names), progress: null, status: "running" }, ...ts]);
    try {
      const results = await run(id);
      updateTransfer(id, { status: "done", results: results ?? undefined });
    } catch (e) {
      failTransfer(id, e);
    }
  };

  const upload = async (localPaths: string[], remoteDir: string) => {
    if (sessionId == null || localPaths.length === 0) return;
    const start = () =>
      runTransfer("upload", localPaths.map(basename), async (id) => {
        await sftp.upload(sessionId, id, localPaths, remoteDir, (progress) => updateTransfer(id, { progress }));
        if (cwdRef.current === remoteDir) void load(remoteDir);
      });
    const names =
      remoteDir === cwd
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
    void runTransfer(
      "download",
      targets.map((e) => e.name),
      (id) => sftp.download(sessionId, id, targets.map((e) => e.path), localDir, (progress) => updateTransfer(id, { progress })),
    );
  };

  /** A single file is saved under a name of the user's choosing; anything else into a folder. */
  const downloadTo = async (targets: FileEntry[]) => {
    if (sessionId == null || targets.length === 0) return;
    const last = storedDownloadTo();
    const [first] = targets;
    if (targets.length === 1 && !first.isDir) {
      const defaultPath = last ? await join(last, first.name).catch(() => first.name) : first.name;
      const path = await save({ defaultPath, title: t("sftp.downloadToTitle") }).catch(() => null);
      if (!path) return;
      storeDownloadTo(await dirname(path));
      void runTransfer("download", [first.name], async (id) => [
        await sftp.downloadAs(sessionId, id, first.path, path, (progress) => updateTransfer(id, { progress })),
      ]);
      return;
    }
    const dir = await open({ directory: true, defaultPath: last ?? undefined, title: t("sftp.downloadToFolderTitle") }).catch(
      () => null,
    );
    if (typeof dir !== "string") return;
    storeDownloadTo(dir);
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
    if (sessionId == null || !connected) return updateTransfer(id, { save: { state: "waiting" } });
    if (editUploads.current.has(id)) return void editUploads.current.set(id, true);
    editUploads.current.set(id, false);
    updateTransfer(id, { save: { state: "uploading" } });
    try {
      await sftp.editUpload(sessionId, id, force);
      updateTransfer(id, { save: { state: "uploaded", at: Date.now() } });
      const dir = cwdRef.current;
      if (transfer.remotePath && dir === parentPath(transfer.remotePath)) void load(dir);
    } catch (e) {
      if (errorCode(e) !== "edit.conflict") return updateTransfer(id, { save: { state: "failed", error: errorMessage(e) } });
      // Until the user decides.
      updateTransfer(id, { save: { state: "skipped" } });
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
    setTransfers((ts) => [
      { id, kind: "edit", label: entry.name, remotePath: entry.path, progress: null, status: "running" },
      ...ts,
    ]);
    try {
      await sftp.editOpen(
        sessionId,
        id,
        entry.path,
        (progress) => updateTransfer(id, { progress }),
        () => void uploadEditRef.current(id),
      );
      updateTransfer(id, { status: "editing" });
    } catch (e) {
      failTransfer(id, e);
    }
  };

  const stopEditing = (id: string) => {
    void sftp.editStop(id);
    setTransfers((ts) => ts.filter((t) => t.id !== id));
  };

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
    if (cwdRef.current) void load(cwdRef.current);
  };

  // Dragging out: the items are downloaded only once dropped on another application (into a
  // folder there, or a temporary one on Windows); dropped on a folder here, they are moved.
  const startDrag = async (items: FileEntry[]) => {
    const { sessionId, connected } = sessionRef.current;
    if (sessionId == null || !connected || items.length === 0) return;
    const id = crypto.randomUUID();
    const paths = items.map((entry) => entry.path);
    let listed = false;
    draggingRef.current = paths;
    try {
      const result = await sftp.dragOut(sessionId, id, items, (event) => {
        if (!listed) {
          listed = true;
          setTransfers((ts) => [
            { id, kind: "download", label: labelFor(items.map((entry) => entry.name)), progress: null, status: "running" },
            ...ts,
          ]);
        }
        if (event.type === "progress") updateTransfer(id, { progress: event });
        else if (event.type === "done") updateTransfer(id, { status: "done", results: event.paths });
        else failTransfer(id, event.error);
      });
      const dir = result.outcome === "inside" ? folderAt(result.x, result.y) : undefined;
      if (dir && !isPointlessMove(paths, dir)) void move(paths, dir);
    } catch (e) {
      fail(e);
    } finally {
      dropDirRef.current = null;
      setDropDir(null);
      // Drop events may arrive after the drag has ended (on Windows they all do); they must
      // not be taken for files dropped from another application.
      setTimeout(() => {
        if (draggingRef.current === paths) draggingRef.current = null;
      }, 1000);
    }
  };
  const startDragRef = useRef(startDrag);
  startDragRef.current = startDrag;

  useEffect(() => {
    const onMove = (e: globalThis.MouseEvent) => {
      const press = pressRef.current;
      if (!press) return;
      if ((e.buttons & 1) === 0) {
        pressRef.current = null;
        return;
      }
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_DISTANCE) return;
      pressRef.current = null;
      const selected = selectedRef.current;
      const pressed = entriesRef.current.find((entry) => entry.path === press.path);
      if (!pressed) return;
      void startDragRef.current(selected.some((entry) => entry.path === press.path) ? selected : [pressed]);
    };
    const onUp = () => (pressRef.current = null);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  /** The folder at a point in the page: null on the panel elsewhere, undefined off the panel. */
  const folderAt = (x: number, y: number) => {
    const element = document.elementFromPoint(x, y);
    if (!element || !panelRef.current?.contains(element)) return undefined;
    return element.closest<HTMLElement>("[data-drop-dir]")?.dataset.dropDir ?? null;
  };

  const pickAndUpload = async (directory: boolean) => {
    if (!cwd) return;
    const picked = await open({ multiple: true, directory, title: directory ? t("sftp.chooseFolders") : t("sftp.chooseFiles") });
    if (picked) void upload(Array.isArray(picked) ? picked : [picked], cwd);
  };

  // Drops while this panel is visible: files from Finder / Explorer are uploaded (into the
  // folder under the pointer, else the current one); our own drags only track the folder.
  const uploadRef = useRef(upload);
  uploadRef.current = upload;
  useEffect(() => {
    if (!active) return;
    // Physical pixels on Windows; on macOS wry passes points despite the type.
    const scale = isMac ? 1 : window.devicePixelRatio;
    const folderUnder = (pos: { x: number; y: number }) => folderAt(pos.x / scale, pos.y / scale);
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      const dragging = draggingRef.current;
      if (payload.type === "leave") {
        setDragOver(false);
        dropDirRef.current = null;
        setDropDir(null);
      } else if (payload.type === "enter" || payload.type === "over") {
        const folder = folderUnder(payload.position);
        const dir = folder && !(dragging && isPointlessMove(dragging, folder)) ? folder : null;
        dropDirRef.current = dir;
        setDropDir(dir);
        setDragOver(!dragging && folder !== undefined);
      } else if (payload.type === "drop") {
        setDragOver(false);
        setDropDir(null);
        if (dragging) return;
        const folder = folderUnder(payload.position);
        const dir = folder ?? cwdRef.current;
        if (folder !== undefined && dir) void uploadRef.current(payload.paths, dir);
      }
    });
    return () => void unlisten.then((f) => f());
  }, [active]);

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
      await load(cwd);
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
      await load(cwd);
    } catch (err) {
      fail(err);
    }
  };

  const submitChmod = async (e: FormEvent) => {
    e.preventDefault();
    if (!chmodTarget || sessionId == null || !cwd) return;
    if (!/^[0-7]{3,4}$/.test(chmodTarget.value)) {
      setError(t("sftp.chmodInvalid"));
      return;
    }
    const { entry, value } = chmodTarget;
    setChmodTarget(null);
    try {
      await sftp.chmod(sessionId, entry.path, parseInt(value, 8));
      await load(cwd);
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
        if (sessionId == null || !cwd) return;
        for (const entry of targets) {
          try {
            await sftp.remove(sessionId, entry.path);
          } catch (err) {
            fail(err);
            break;
          }
        }
        await load(cwd);
      },
    });
  };

  const startRename = (entry: FileEntry) => setRenaming({ path: entry.path, value: entry.name });
  const startChmod = (entry: FileEntry) =>
    setChmodTarget({ entry, value: ((entry.permissions ?? 0o644) & 0o7777).toString(8).padStart(3, "0") });

  const activate = (entry: FileEntry) => (entry.isDir ? void load(entry.path) : download([entry]));

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

  const onRowMouseDown = (e: MouseEvent, entry: FileEntry) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("input")) return;
    if (e.shiftKey && anchor) {
      const range = pathRange(visible, anchor, entry.path);
      setSelection(new Set(isMod(e) ? [...selection, ...range] : range));
      setCursor(entry.path);
    } else if (isMod(e)) {
      const next = new Set(selection);
      if (next.has(entry.path)) next.delete(entry.path);
      else next.add(entry.path);
      setSelection(next);
      setAnchor(entry.path);
      setCursor(entry.path);
    } else {
      // A press on a selected row keeps the selection, so that it can be dragged together.
      if (!selection.has(entry.path)) select([entry.path]);
      pressRef.current = { x: e.clientX, y: e.clientY, path: entry.path };
    }
  };

  // WebKit also counts a click on the header (or another row) followed quickly by one on a
  // row as a double click.
  const isDoubleClick = (e: MouseEvent, entry: FileEntry) =>
    !e.shiftKey && !isMod(e) && pressedRowsRef.current.every((path) => path === entry.path);

  const onRowClick = (e: MouseEvent, entry: FileEntry) => {
    if (!e.shiftKey && !isMod(e) && selection.size > 1) select([entry.path]);
  };

  const fileMenu = (targets: FileEntry[]): MenuItem[] => {
    const one = targets.length === 1 ? targets[0] : null;
    const offline = !connected;
    const items: MenuItem[] = [];
    if (one?.isDir) items.push({ label: t("sftp.open"), disabled: offline, onSelect: () => void load(one.path) });
    items.push(
      { label: t("sftp.download"), disabled: offline, onSelect: () => download(targets) },
      { label: t("sftp.downloadTo"), disabled: offline, onSelect: () => void downloadTo(targets) },
    );
    if (one && !one.isDir) items.push({ label: t("sftp.openInEditor"), disabled: offline, onSelect: () => void openInEditor(one) });
    items.push("separator");
    if (one) {
      items.push(
        { label: t("sftp.rename"), shortcut: isMac ? undefined : "F2", disabled: offline, onSelect: () => startRename(one) },
        { label: t("sftp.changePermissions"), disabled: offline, onSelect: () => startChmod(one) },
      );
    }
    items.push(
      {
        label: targets.length > 1 ? t("sftp.copyPaths") : t("sftp.copyPath"),
        onSelect: () =>
          void writeText(targets.map((e) => e.path).join("\n")).then(() => announce(t("announce.copied")), fail),
      },
      "separator",
      {
        label: t("sftp.delete"),
        shortcut: isMac ? "⌫" : "Del",
        danger: true,
        disabled: offline,
        onSelect: () => askRemove(targets),
      },
    );
    return items;
  };

  const folderMenu = (): MenuItem[] => [
    { label: t("sftp.uploadFiles"), disabled: !connected || !cwd, onSelect: () => void pickAndUpload(false) },
    { label: t("sftp.uploadFolder"), disabled: !connected || !cwd, onSelect: () => void pickAndUpload(true) },
    { label: t("sftp.newFolder"), disabled: !connected || !cwd, onSelect: () => setNewFolder("") },
    "separator",
    { label: t("sftp.refresh"), disabled: !connected || !cwd, onSelect: () => void refresh() },
    {
      label: view.showHidden ? t("sftp.hideHidden") : t("sftp.showHidden"),
      onSelect: () => changeView({ showHidden: !view.showHidden }),
    },
  ];

  const onRowContextMenu = (e: MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();
    const targets = selection.has(entry.path) ? selected : [entry];
    if (!selection.has(entry.path)) select([entry.path]);
    setMenu({ x: e.clientX, y: e.clientY, items: fileMenu(targets) });
  };

  const onListContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    select([]);
    setMenu({ x: e.clientX, y: e.clientY, items: folderMenu() });
  };

  const moveCursor = (index: number, extend: boolean) => {
    if (visible.length === 0) return;
    const path = visible[Math.max(0, Math.min(visible.length - 1, index))].path;
    if (extend && anchor) {
      setSelection(new Set(pathRange(visible, anchor, path)));
      setCursor(path);
    } else {
      select([path]);
    }
    listRef.current?.querySelector(`tr[data-path="${CSS.escape(path)}"]`)?.scrollIntoView({ block: "nearest" });
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
      else download(selected);
    } else if ((e.key === "Delete" || e.key === "Backspace") && connected) askRemove(selected);
    else if (e.key === "F2" && selected.length === 1 && connected) startRename(selected[0]);
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
        {label}
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
          onClick={() => parent && load(parent)}
        >
          <ArrowIcon direction="up" />
        </IconButton>
        <IconButton className="icon-button" label={t("sftp.refresh")} disabled={!cwd} onClick={refresh}>
          <RefreshIcon />
        </IconButton>
        <form
          className="sftp-path"
          onSubmit={(e) => {
            e.preventDefault();
            void load(pathInput.trim() || ".");
          }}
        >
          <input value={pathInput} onChange={(e) => setPathInput(e.target.value)} spellCheck={false} title={t("sftp.pathHint")} />
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
        <div className="sftp-error" onClick={() => setError(null)} title={t("sftp.dismissHint")}>
          {error}
        </div>
      )}
      {!connected && <div className="sftp-error">{t("sftp.disconnected")}</div>}

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
                aria-selected={selection.has(entry.path)}
                data-path={entry.path}
                data-drop-dir={entry.isDir ? entry.path : undefined}
                className={
                  [selection.has(entry.path) && "selected", dropDir === entry.path && "drop-target"].filter(Boolean).join(" ") ||
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
                    <span className="file-icon" aria-hidden="true">
                      {entry.isDir ? "📁" : "📄"}
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
                            if (e.key !== "Escape") return;
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
        transfers={transfers}
        onCancel={(id) => void sftp.cancel(id)}
        onDismiss={(id) => setTransfers((ts) => ts.filter((t) => t.id !== id))}
        onClearFinished={() => setTransfers((ts) => ts.filter((t) => t.status === "running" || t.status === "editing"))}
        onReopen={(id) => void sftp.editReopen(id).catch(fail)}
        onStopEditing={stopEditing}
      />

      {dragOver && (
        <div className="drop-hint">{t("sftp.dropHint", { path: dropDir ?? cwd ?? "" })}</div>
      )}

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {chmodTarget && (
        <ChmodDialog onClose={() => setChmodTarget(null)}>
          <form className="dialog" onSubmit={submitChmod}>
            <h2>{t("sftp.chmodTitle")}</h2>
            <label>
              {t("sftp.chmodLabel", { name: chmodTarget.entry.name })}
              <input
                autoFocus
                value={chmodTarget.value}
                onChange={(e) => setChmodTarget({ ...chmodTarget, value: e.target.value })}
              />
            </label>
            <p className="hint">{/^[0-7]{3,4}$/.test(chmodTarget.value) && formatMode(parseInt(chmodTarget.value, 8), chmodTarget.entry.isDir, false)}</p>
            <footer>
              <span className="grow" />
              <button type="button" onClick={() => setChmodTarget(null)}>
                {t("common.cancel")}
              </button>
              <button type="submit" className="primary">
                {t("common.ok")}
              </button>
            </footer>
          </form>
        </ChmodDialog>
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
