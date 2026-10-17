import { useCallback, useEffect, useEffectEvent, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { announce } from "../../lib/announce";
import { sftp, type SessionId } from "../../ipc/api";
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
  /** The shell's folder when the panel last went there (or opened there), from `useShellDir`. */
  syncedRef: RefObject<string | null>;
  load(path: string, quiet?: boolean): Promise<boolean>;
  setError(message: string | null): void;
}

/**
 * The folder the terminal's shell is in: found by the backend where the server allows (Linux),
 * otherwise as the shell last reported it (OSC 7).
 */
const findShellDir = async (id: SessionId, paneKey: number) => (await sftp.shellDir(id).catch(() => null)) ?? shellDirs.reported(paneKey);

/**
 * Where the file panel of pane `paneKey` opens (`startDir`: the shell's folder), and
 * `syncedRef`, the shell's folder when the panel last went there, which `useShellFollow` goes
 * on from. Apart from it because the listing opens the panel, and following needs the listing.
 */
export function useShellDir(paneKey: number) {
  const syncedRef = useRef<string | null>(null);
  const startDir = useCallback(
    async (id: SessionId) => {
      const dir = await findShellDir(id, paneKey);
      syncedRef.current = dir;
      return dir;
    },
    [paneKey],
  );
  return { startDir, syncedRef };
}

/**
 * The file panel going to the folder its terminal's shell is in (see `findShellDir`). While
 * following, the panel goes to the shell's folder each time the shell may have changed folders
 * and has: browsing elsewhere meanwhile isn't undone until the shell moves again.
 */
export function useShellFollow({ paneKey, sessionId, connected, active, follow, cwdRef, syncedRef, load, setError }: Options) {
  const { t } = useTranslation();
  const find = useCallback((id: SessionId) => findShellDir(id, paneKey), [paneKey]);

  /** Goes to the shell's folder now (the toolbar button, turning following on). */
  const goToShellDir = useCallback(async () => {
    if (sessionId == null) return;
    const dir = await find(sessionId);
    if (!dir) {
      setError(t("sftp.shellDirUnknown"));
      return;
    }
    if (await load(dir)) syncedRef.current = dir;
  }, [sessionId, find, load, setError, syncedRef, t]);

  const loadFollowed = useEffectEvent(load);
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
          if (dir === cwdRef.current) syncedRef.current = dir;
          // Only once there: a folder that couldn't be listed (an error, or a reconnection
          // meanwhile) is tried again the next time the shell may have moved.
          else if ((await loadFollowed(dir, true)) && live) {
            syncedRef.current = dir;
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
  }, [follow, active, connected, sessionId, paneKey, find, cwdRef, syncedRef, t]);

  return { goToShellDir };
}
