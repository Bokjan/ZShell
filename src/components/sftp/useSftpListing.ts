import { useCallback, useEffect, useRef, useState } from "react";

import { errorMessage, sftp, type FileEntry, type SessionId } from "../../ipc/api";

/**
 * The folder the file panel shows and its entries. `onNewFolder` runs when another folder is
 * shown (not when the same one is listed again), to start its selection afresh. The panel
 * opens in `startDir` (the terminal's folder), or in the remote home folder without one or if
 * it can't be listed.
 */
export function useSftpListing(
  sessionId: SessionId | null,
  connected: boolean,
  onNewFolder: () => void,
  startDir: (id: SessionId) => Promise<string | null>,
) {
  const [cwd, setCwd] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cwdRef = useRef<string | null>(null);
  cwdRef.current = cwd;
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;
  const onNewFolderRef = useRef(onNewFolder);
  onNewFolderRef.current = onNewFolder;
  const startDirRef = useRef(startDir);
  startDirRef.current = startDir;
  /**
   * Whether the user is typing in the path field: a listing they didn't ask for there (after
   * an upload, or following the terminal) leaves what they typed alone.
   */
  const editingRef = useRef(false);

  const fail = useCallback((e: unknown) => setError(errorMessage(e)), []);

  /** Counts listings asked for: only the latest one's answer is shown. */
  const seqRef = useRef(0);
  /** Shows the folder at `path`; resolves to whether it did. `quiet`: a failure shows no error. */
  const load = useCallback(
    async (path: string, quiet = false) => {
      if (sessionId == null) return false;
      const seq = ++seqRef.current;
      // A slower answer for a folder opened before, or from the connection before a
      // reconnection, must not replace what came after it.
      const stale = () => seq !== seqRef.current || sessionRef.current !== sessionId;
      setLoading(true);
      try {
        const listing = await sftp.list(sessionId, path);
        if (stale()) return false;
        // Refreshing keeps the selection; another folder starts afresh.
        if (listing.path !== cwdRef.current) onNewFolderRef.current();
        setCwd(listing.path);
        if (!editingRef.current) setPathInput(listing.path);
        setEntries(listing.entries);
        setError(null);
        return true;
      } catch (e) {
        if (stale() || quiet) return false;
        if (!editingRef.current) setPathInput(cwdRef.current ?? "");
        fail(e);
        return false;
      } finally {
        if (seq === seqRef.current) setLoading(false);
      }
    },
    [sessionId, fail],
  );

  /**
   * Lists the folder shown now again: after an operation, which may have taken long enough
   * (deleting a large folder) for the user to have gone to another folder meanwhile.
   */
  const reload = useCallback(() => {
    if (cwdRef.current) void load(cwdRef.current);
  }, [load]);

  // (Re)open on connect, staying in the current directory across reconnects. An answer that
  // comes after a reconnection is dropped: its listing would make the newer one stale, and
  // its error would show over it.
  useEffect(() => {
    if (sessionId == null) return;
    if (!connected) {
      // A listing still on its way is the lost connection's: it would show the panel as
      // connected again, or an error over the disconnected notice.
      seqRef.current++;
      setLoading(false);
      return;
    }
    if (cwdRef.current) {
      void load(cwdRef.current);
      return;
    }
    let live = true;
    const open = async () => {
      const home = await sftp.open(sessionId);
      const start = live ? await startDirRef.current(sessionId).catch(() => null) : null;
      if (live && !(start && (await load(start, true))) && live) await load(home);
    };
    open().catch((e) => {
      if (live) fail(e);
    });
    return () => {
      live = false;
    };
  }, [sessionId, connected, load, fail]);

  /** Whether the path field is being edited, which a listing then leaves alone. */
  const setEditing = (editing: boolean) => {
    editingRef.current = editing;
  };

  return { cwd, cwdRef, pathInput, setPathInput, setEditing, entries, loading, error, setError, fail, load, reload };
}
