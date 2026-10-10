import { useEffect, useRef, useState, type MouseEvent, type RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { sftp, type FileEntry, type SessionId } from "../../lib/api";
import { usePressDrag } from "../../lib/drag";
import { isMac } from "../../lib/platform";
import type { Transfer } from "../TransferList";
import { parentPath } from "./useTransfers";

const DRAG_DISTANCE = 5;

/** Whether moving `paths` into `dir` would do nothing or put a folder inside itself. */
const isPointlessMove = (paths: string[], dir: string) =>
  paths.some((path) => dir === path || dir.startsWith(`${path}/`)) || paths.every((path) => parentPath(path) === dir);

interface Options {
  panelRef: RefObject<HTMLDivElement | null>;
  /** Whether the panel is in sight: only then does it take files dropped on the window. */
  active: boolean;
  sessionId: SessionId | null;
  connected: boolean;
  cwdRef: RefObject<string | null>;
  upload(localPaths: string[], remoteDir: string): Promise<void>;
  /** Moves remote `paths` into folder `dir`. */
  move(paths: string[], dir: string): Promise<void>;
  transfers: {
    add(transfer: Transfer): void;
    update(id: string, patch: Partial<Transfer>): void;
    failed(id: string, e: unknown): void;
    labelFor(names: string[]): string;
  };
  fail(e: unknown): void;
}

/**
 * Dragging in and out of the file panel. Files from Finder / Explorer are uploaded into the
 * folder under the pointer, else the one shown. Rows dragged out are downloaded only once
 * dropped on another application (into a folder there, or a temporary one on Windows), and
 * moved when dropped on a folder here.
 */
export function useSftpDragDrop(options: Options) {
  const [dragOver, setDragOver] = useState(false);
  /** The folder that what is being dragged would go into. */
  const [dropDir, setDropDir] = useState<string | null>(null);
  /** The paths being dragged out of this panel, until shortly after the drag ends. */
  const draggingRef = useRef<string[] | null>(null);
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const pressDrag = usePressDrag();

  /** The folder at a point in the page: null on the panel elsewhere, undefined off the panel. */
  const folderAt = (x: number, y: number) => {
    const element = document.elementFromPoint(x, y);
    if (!element || !optionsRef.current.panelRef.current?.contains(element)) return undefined;
    return element.closest<HTMLElement>("[data-drop-dir]")?.dataset.dropDir ?? null;
  };

  const startDrag = async (items: FileEntry[]) => {
    const { sessionId, connected, transfers, move, fail } = optionsRef.current;
    if (sessionId == null || !connected || items.length === 0) return;
    const id = crypto.randomUUID();
    const paths = items.map((entry) => entry.path);
    let listed = false;
    draggingRef.current = paths;
    try {
      const result = await sftp.dragOut(sessionId, id, items, (event) => {
        if (!listed) {
          listed = true;
          transfers.add({ id, kind: "download", label: transfers.labelFor(items.map((entry) => entry.name)), progress: null, status: "running" });
        }
        if (event.type === "progress") transfers.update(id, { progress: event });
        else if (event.type === "done") transfers.update(id, { status: "done", results: event.paths });
        else transfers.failed(id, event.error);
      });
      const dir = result.outcome === "inside" ? folderAt(result.x, result.y) : undefined;
      if (dir && !isPointlessMove(paths, dir)) void move(paths, dir);
    } catch (e) {
      fail(e);
    } finally {
      setDropDir(null);
      // Drop events may arrive after the drag has ended (on Windows they all do); they must
      // not be taken for files dropped from another application.
      setTimeout(() => {
        if (draggingRef.current === paths) draggingRef.current = null;
      }, 1000);
    }
  };

  /** A press on a row, which drags `items()` out once the pointer moves far enough. */
  const press = (e: MouseEvent, items: () => FileEntry[]) => {
    let started = false;
    pressDrag(e, {
      threshold: DRAG_DISTANCE,
      onMove: () => {
        // The system takes the drag over from here.
        if (started) return;
        started = true;
        void startDrag(items());
      },
      onEnd: () => {},
    });
  };

  const { active } = options;
  useEffect(() => {
    if (!active) return;
    // Physical pixels on Windows; on macOS wry passes points despite the type.
    const scale = isMac ? 1 : window.devicePixelRatio;
    const folderUnder = (pos: { x: number; y: number }) => folderAt(pos.x / scale, pos.y / scale);
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      const dragging = draggingRef.current;
      if (payload.type === "leave") {
        setDragOver(false);
        setDropDir(null);
      } else if (payload.type === "enter" || payload.type === "over") {
        const folder = folderUnder(payload.position);
        setDropDir(folder && !(dragging && isPointlessMove(dragging, folder)) ? folder : null);
        setDragOver(!dragging && folder !== undefined);
      } else if (payload.type === "drop") {
        setDragOver(false);
        setDropDir(null);
        if (dragging) return;
        const folder = folderUnder(payload.position);
        const dir = folder ?? optionsRef.current.cwdRef.current;
        if (folder !== undefined && dir) void optionsRef.current.upload(payload.paths, dir);
      }
    });
    return () => void unlisten.then((f) => f());
  }, [active]);

  return { dragOver, dropDir, press };
}
