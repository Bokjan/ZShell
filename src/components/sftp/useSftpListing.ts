import { useCallback, useEffect, useRef, useState } from "react";

import { errorMessage, sftp, type FileEntry, type SessionId } from "../../lib/api";

/**
 * The folder the file panel shows and its entries. `onNewFolder` runs when another folder is
 * shown (not when the same one is listed again), to start its selection afresh.
 */
export function useSftpListing(sessionId: SessionId | null, connected: boolean, onNewFolder: () => void) {
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

  const fail = useCallback((e: unknown) => setError(errorMessage(e)), []);

  /** Counts listings asked for: only the latest one's answer is shown. */
  const seqRef = useRef(0);
  const load = useCallback(
    async (path: string) => {
      if (sessionId == null) return;
      const seq = ++seqRef.current;
      // A slower answer for a folder opened before, or from the connection before a
      // reconnection, must not replace what came after it.
      const stale = () => seq !== seqRef.current || sessionRef.current !== sessionId;
      setLoading(true);
      try {
        const listing = await sftp.list(sessionId, path);
        if (stale()) return;
        // Refreshing keeps the selection; another folder starts afresh.
        if (listing.path !== cwdRef.current) onNewFolderRef.current();
        setCwd(listing.path);
        setPathInput(listing.path);
        setEntries(listing.entries);
        setError(null);
      } catch (e) {
        if (stale()) return;
        setPathInput(cwdRef.current ?? "");
        fail(e);
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

  // (Re)open on connect, staying in the current directory across reconnects.
  useEffect(() => {
    if (sessionId == null || !connected) return;
    if (cwdRef.current) void load(cwdRef.current);
    else sftp.open(sessionId).then(load, fail);
  }, [sessionId, connected, load, fail]);

  return { cwd, cwdRef, pathInput, setPathInput, entries, loading, error, setError, fail, load, reload };
}
