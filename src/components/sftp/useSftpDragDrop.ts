import { useEffect, useRef, useState, type MouseEvent, type RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { sftp, type FileEntry, type SessionId } from "../../ipc/api";
import { windows } from "../../ipc/windows";
import { isAppLocked } from "../../lib/appLock";
import { usePressDrag } from "../../hooks/drag";
import { isMac, isWindows } from "../../lib/platform";
import type { Transfer } from "../TransferList";
import { attempt } from "../../lib/attempt";
import { useLatest } from "../../hooks/latest";
import { parentPath } from "./useTransfers";

const DRAG_DISTANCE = 5;
/** How near the top or bottom of the list's rows a drag scrolls it, in pixels. */
const SCROLL_EDGE = 24;
/** How far it scrolls each frame with the pointer at the very edge, in pixels. */
const SCROLL_SPEED = 12;

/** Whether moving `paths` into `dir` would do nothing or put a folder inside itself. */
const isPointlessMove = (paths: string[], dir: string) =>
  paths.some((path) => dir === path || dir.startsWith(`${path}/`)) || paths.every((path) => parentPath(path) === dir);

interface Options {
  panelRef: RefObject<HTMLDivElement | null>;
  /** The file list, whose rows are dragged out. */
  listRef: RefObject<HTMLDivElement | null>;
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
 * folder under the pointer, else the one shown; while they are over the panel (`dragOver`),
 * it shows a drop zone for the one shown, which the list's folders may leave no room for.
 * Rows dragged out are downloaded only once dropped on another application (into a folder
 * there, or a temporary one on Windows), and moved when dropped on a folder here. Either drag
 * scrolls the list near the top or bottom of its rows, to reach the folders out of sight.
 */
export function useSftpDragDrop(options: Options) {
  const [dragOver, setDragOver] = useState(false);
  /** The folder that what is being dragged would go into. */
  const [dropDir, setDropDir] = useState<string | null>(null);
  /** The paths being dragged out of this panel, until shortly after the drag ends. */
  const draggingRef = useRef<string[] | null>(null);
  const optionsRef = useLatest(options);
  const pressDrag = usePressDrag();

  /** The folder at a point in the page: null on the panel elsewhere, undefined off the panel. */
  const folderAt = (x: number, y: number) => {
    const element = document.elementFromPoint(x, y);
    if (!element || !optionsRef.current.panelRef.current?.contains(element)) return undefined;
    return element.closest<HTMLElement>("[data-drop-dir]")?.dataset.dropDir ?? null;
  };

  const startDrag = async (items: FileEntry[], pressedAt: number) => {
    const { sessionId, connected, transfers, move, fail } = optionsRef.current;
    if (sessionId == null || !connected || items.length === 0) return;
    const id = crypto.randomUUID();
    const paths = items.map((entry) => entry.path);
    let listed = false;
    draggingRef.current = paths;
    return attempt(
      async () => {
        const result = await sftp.dragOut(sessionId, id, items, pressedAt, (event) => {
          if (!listed) {
            listed = true;
            transfers.add({
              id,
              kind: "download",
              label: transfers.labelFor(items.map((entry) => entry.name)),
              progress: null,
              status: "running",
            });
          }
          if (event.type === "progress") transfers.update(id, { progress: event });
          else if (event.type === "done") transfers.update(id, { status: "done", results: event.paths });
          else transfers.failed(id, event.error);
        });
        const dir = result.outcome === "inside" ? folderAt(result.x, result.y) : undefined;
        if (dir && !isPointlessMove(paths, dir)) void move(paths, dir);
      },
      {
        onError: (e) => {
          fail(e);
        },
        cleanup: () => {
          setDropDir(null);
          // Drop events may arrive after the drag has ended (on Windows they all do); they must
          // not be taken for files dropped from another application.
          setTimeout(() => {
            if (draggingRef.current === paths) draggingRef.current = null;
          }, 1000);
        },
      },
    );
  };

  /** A press on a row, which drags `items()` out once the pointer moves far enough. */
  const press = (e: MouseEvent, items: () => FileEntry[]) => {
    const pressedAt = e.timeStamp;
    let started = false;
    pressDrag(e, {
      threshold: DRAG_DISTANCE,
      onMove: () => {
        // The system takes the drag over from here.
        if (started) return;
        started = true;
        void startDrag(items(), pressedAt);
      },
      onEnd: () => {},
    });
  };

  const { active } = options;

  // On macOS a press on the rows of a window in the background reaches the page without
  // bringing the window forward, so that they can be dragged to the window in front (see
  // `window_set_drag_area`); released without dragging, it brings the window forward here.
  // The area is placed again as the window goes to the background, in case the layout moved
  // the list without resizing it.
  useEffect(() => {
    const list = optionsRef.current.listRef.current;
    if (!active || !isMac || !list) return;
    const place = () => {
      const r = list.getBoundingClientRect();
      const top = Math.max(r.top, list.querySelector("thead")?.getBoundingClientRect().bottom ?? r.top);
      const rect = { x: r.left, y: top, width: r.width, height: Math.max(0, r.bottom - top) };
      windows.setDragArea(rect);
    };
    const onMouseDown = (e: globalThis.MouseEvent) => {
      if (e.button !== 0 || document.hasFocus()) return;
      const onMouseUp = () => {
        window.removeEventListener("mouseup", onMouseUp, true);
        if (!draggingRef.current) void getCurrentWindow().setFocus().catch(console.error);
      };
      window.addEventListener("mouseup", onMouseUp, true);
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(list);
    window.addEventListener("blur", place);
    list.addEventListener("mousedown", onMouseDown, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("blur", place);
      list.removeEventListener("mousedown", onMouseDown, true);
      windows.setDragArea(null);
    };
  }, [active]);

  useEffect(() => {
    if (!active) return;
    // Physical pixels on Windows; on macOS (points) and Linux (GTK's logical pixels) wry
    // passes CSS pixels despite the type.
    const scale = isWindows ? window.devicePixelRatio : 1;
    /** Where the drag is, in CSS pixels; null once it has left or ended. */
    let pointer: { x: number; y: number } | null = null;
    let frame = 0;

    /** Shows where what is dragged would go with the pointer where it is. */
    const hover = () => {
      if (!pointer) return;
      const dragging = draggingRef.current;
      const folder = folderAt(pointer.x, pointer.y);
      setDropDir(folder && !(dragging && isPointlessMove(dragging, folder)) ? folder : null);
      setDragOver(!dragging && folder !== undefined);
    };

    /** Scrolls the list while the pointer is near the top or bottom of its rows, faster nearer. */
    const scroll = () => {
      frame = 0;
      const list = optionsRef.current.listRef.current;
      if (!pointer || !list) return;
      const r = list.getBoundingClientRect();
      // Below the header, which stays at the top.
      const top = Math.max(r.top, list.querySelector("thead")?.getBoundingClientRect().bottom ?? r.top);
      const { x, y } = pointer;
      if (x < r.left || x > r.right || y < top || y > r.bottom) return;
      const depth = y < top + SCROLL_EDGE ? y - top - SCROLL_EDGE : y > r.bottom - SCROLL_EDGE ? y - r.bottom + SCROLL_EDGE : 0;
      if (depth === 0) return;
      const before = list.scrollTop;
      list.scrollTop += Math.round((depth / SCROLL_EDGE) * SCROLL_SPEED) || Math.sign(depth);
      if (list.scrollTop === before) return;
      hover();
      frame = requestAnimationFrame(scroll);
    };

    const end = () => {
      pointer = null;
      cancelAnimationFrame(frame);
      frame = 0;
      setDragOver(false);
      setDropDir(null);
    };

    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      // Native events, which the lock screen doesn't stop: nothing is dropped while locked.
      if (isAppLocked() || payload.type === "leave") {
        end();
      } else if (payload.type === "enter" || payload.type === "over") {
        pointer = { x: payload.position.x / scale, y: payload.position.y / scale };
        hover();
        if (!frame) frame = requestAnimationFrame(scroll);
      } else if (payload.type === "drop") {
        end();
        if (draggingRef.current) return;
        const folder = folderAt(payload.position.x / scale, payload.position.y / scale);
        const dir = folder ?? optionsRef.current.cwdRef.current;
        if (folder !== undefined && dir) void optionsRef.current.upload(payload.paths, dir);
      }
    });
    return () => {
      end();
      void unlisten.then((f) => f());
    };
  }, [active]);

  return { dragOver, dropDir, press };
}
