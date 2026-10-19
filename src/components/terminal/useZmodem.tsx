import { useEffect, useInsertionEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { zmodem, type ZmodemPhase } from "../../ipc/api";
import { open as openDialog } from "../../ipc/fileDialog";
import { isAppLocked, whenUnlocked } from "../../lib/appLock";
import type { PaneSession } from "../../lib/paneSession";
import { useSettings } from "../../hooks/settings";
import { ZmodemBar } from "../ZmodemBar";

interface Options {
  /** The pane's session, for answering transfers. */
  sessionRef: RefObject<PaneSession | null>;
  /** Shown and focused: files dropped on the terminal go to its transfer. */
  active: boolean;
  /** The terminal's area, where files are dropped. */
  wrapRef: RefObject<HTMLDivElement | null>;
  /** Gives the terminal the keyboard back after a file dialog. */
  focus(): void;
}

interface Zmodem {
  /** Called with each phase the session reports. */
  onPhaseRef: RefObject<(phase: ZmodemPhase) => void>;
  /** Forgets the transfer: the pane has another session. */
  reset(): void;
  /** The bar shown while a transfer waits for files or runs. */
  bar: ReactNode;
}

/** A terminal's ZMODEM transfers (`rz` and `sz`): the bar, the file dialogs and dropped files. */
export function useZmodem({ sessionRef, active, wrapRef, focus }: Options): Zmodem {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const onPhaseRef = useRef<(phase: ZmodemPhase) => void>(() => {});
  // A transfer waiting for files or running; null when there is none.
  const [phase, setPhase] = useState<Exclude<ZmodemPhase, "idle"> | null>(null);
  const [dragOver, setDragOver] = useState(false);

  // ZMODEM: `sz` asks where to save: the bar asks (download folder, another folder or
  // cancel) unless the settings say to use the download folder, or to open the folder picker
  // right away. `rz` asks for files: the picker opens right away. Either way the bar stays,
  // for choosing again (or dropping files) or cancelling, after the picker is closed.
  const chooseFiles = () => {
    const id = sessionRef.current?.id;
    if (id == null) return;
    void openDialog({ multiple: true, title: t("zmodem.chooseFilesTitle") }).then((picked) => {
      if (picked && picked.length > 0) void zmodem.sendFiles(id, picked).catch(console.error);
      focus();
    });
  };

  const saveReceived = (dir: string | null) => {
    const id = sessionRef.current?.id;
    if (id != null) void zmodem.saveTo(id, dir).catch(console.error);
  };

  const chooseFolder = () => {
    void openDialog({ directory: true, title: t("zmodem.chooseFolderTitle") }).then((dir) => {
      if (typeof dir === "string") saveReceived(dir);
      focus();
    });
  };

  const cancel = () => {
    const id = sessionRef.current?.id;
    if (id != null) void zmodem.cancel(id).catch(console.error);
  };

  // A file dialog that `rz` or `sz` opened while the app was locked, waiting to be shown:
  // shown at once, it would be over the lock screen.
  const pendingDialogRef = useRef<() => void>(null);
  useInsertionEffect(() => {
    onPhaseRef.current = (phase) => {
      const id = sessionRef.current?.id;
      if (id == null) return;
      setPhase(phase === "idle" ? null : phase);
      pendingDialogRef.current?.();
      pendingDialogRef.current = null;
      if (phase === "chooseFiles") pendingDialogRef.current = whenUnlocked(chooseFiles);
      else if (phase === "chooseDestination") {
        if (settings.zmodem.receive === "downloads") saveReceived(null);
        else if (settings.zmodem.receive === "chooseFolder") pendingDialogRef.current = whenUnlocked(chooseFolder);
      }
    };
  });

  // While `rz` waits, files dropped on this terminal are sent.
  const waitingForFiles = active && phase === "chooseFiles";
  useEffect(() => {
    if (!waitingForFiles) return;
    const inside = (pos: { x: number; y: number }) => {
      const rect = wrapRef.current?.getBoundingClientRect();
      const x = pos.x / window.devicePixelRatio;
      const y = pos.y / window.devicePixelRatio;
      return !!rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    };
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      // Native events, which the lock screen doesn't stop: nothing is dropped while locked.
      if (isAppLocked()) {
        setDragOver(false);
        return;
      }
      if (payload.type === "leave") setDragOver(false);
      else if (payload.type === "enter" || payload.type === "over") setDragOver(inside(payload.position));
      else if (payload.type === "drop") {
        setDragOver(false);
        const id = sessionRef.current?.id;
        if (inside(payload.position) && id != null && payload.paths.length > 0) {
          void zmodem.sendFiles(id, payload.paths).catch(console.error);
        }
      }
    });
    return () => {
      setDragOver(false);
      void unlisten.then((f) => f());
    };
  }, [waitingForFiles]);

  const bar = phase && (
    <ZmodemBar
      phase={phase}
      dragOver={dragOver}
      onChooseFiles={chooseFiles}
      onSaveToDownloads={() => saveReceived(null)}
      onChooseFolder={chooseFolder}
      onCancel={cancel}
    />
  );
  return { onPhaseRef, reset: () => setPhase(null), bar };
}
