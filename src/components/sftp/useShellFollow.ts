import { useCallback, useEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { announce } from "../../lib/announce";
import { sftp, type SessionId } from "../../lib/api";
import { shellDirs } from "../../lib/shellDirs";

interface Options {
  /** The pane whose terminal the panel goes with. */
  paneKey: number;
  sessionId: SessionId | null;
  connected: boolean;
  /** Whether the panel is shown: it follows only then (and catches up when shown again). */
  active: boolean;
  /** Whether it follows the terminal's shell into each folder it goes to. */
  follow: boolean;
  cwdRef: RefObject<string | null>;
  load(path: string, quiet?: boolean): Promise<boolean>;
  setError(message: string | null): void;
}

/**
 * The file panel going to the folder its terminal's shell is in: found by the backend where
 * the server allows (Linux), otherwise as the shell last reported it (OSC 7). `startDir` is
 * where the panel opens. While following, the panel goes to the shell's folder each time the
 * shell may have changed folders and has: browsing elsewhere meanwhile isn't undone until the
 * shell moves again.
 */
export function useShellFollow({ paneKey, sessionId, connected, active, follow, cwdRef, load, setError }: Options) {
  const { t } = useTranslation();
  /** The shell's folder when the panel last went there (or opened there). */
  const syncedRef = useRef<string | null>(null);

  const find = useCallback(async (id: SessionId) => (await sftp.shellDir(id).catch(() => null)) ?? shellDirs.reported(paneKey), [paneKey]);

  const startDir = useCallback(
    async (id: SessionId) => {
      const dir = await find(id);
      syncedRef.current = dir;
      return dir;
    },
    [find],
  );

  /** Goes to the shell's folder now (the toolbar button, turning following on). */
  const goToShellDir = useCallback(async () => {
    if (sessionId == null) return;
    const dir = await find(sessionId);
    if (!dir) {
      setError(t("sftp.shellDirUnknown"));
      return;
    }
    syncedRef.current = dir;
    await load(dir);
  }, [sessionId, find, load, setError, t]);

  const loadRef = useRef(load);
  loadRef.current = load;
  useEffect(() => {
    if (!follow || !active || !connected || sessionId == null) return;
    let live = true;
    // One lookup at a time; a change meanwhile looks again once it is done.
    let running = false;
    let again = false;
    const sync = async () => {
      if (running) {
        again = true;
        return;
      }
      running = true;
      do {
        again = false;
        const dir = await find(sessionId);
        if (!live) return;
        if (dir && dir !== syncedRef.current) {
          syncedRef.current = dir;
          if (dir !== cwdRef.current && (await loadRef.current(dir, true)) && live) {
            announce(t("sftp.followedTerminal", { path: dir }));
          }
        }
      } while (again && live);
      running = false;
    };
    // The shell may have moved while the panel was hidden.
    void sync();
    const stop = shellDirs.subscribe(paneKey, () => void sync());
    return () => {
      live = false;
      stop();
    };
  }, [follow, active, connected, sessionId, paneKey, find, cwdRef, t]);

  return { startDir, goToShellDir };
}
