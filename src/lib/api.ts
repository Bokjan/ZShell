import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  CommandError,
  DragItem,
  DragOutEvent,
  DragResult,
  EditEvent,
  ErrorCode,
  FileEntry,
  Folder,
  ForwardRule,
  ImportCandidate,
  KnownHost,
  Listing,
  LogOpen,
  LogSummary,
  Profile,
  Proxy,
  QuickCommands,
  Saved,
  SerialOptions,
  SerialPortInfo,
  SessionCandidate,
  SessionEvent,
  SessionSpec,
  SetAsideFile,
  TransferProgress,
  TreeItem,
} from "./bindings";

export type * from "./bindings";

export const DEFAULT_SERIAL: SerialOptions = {
  device: "",
  baudRate: 115200,
  dataBits: 8,
  parity: "none",
  stopBits: 1,
  flowControl: "none",
};

/** The usual port of each network protocol. */
export const DEFAULT_PORTS = { ssh: 22, telnet: 23 } as const;

/** The character encodings a session can use (WHATWG labels, as the backend stores them). */
export const ENCODINGS = ["utf-8", "gb18030", "gbk", "big5", "shift_jis", "euc-jp", "euc-kr", "windows-1252"] as const;

export const DEFAULT_TERM_TYPE = "xterm-256color";

export type SessionId = number;

/**
 * What a terminal tab runs: a saved session, a quick SSH or Telnet connection without one
 * (SSH: automatic authentication, an empty username is the local user's), or the default
 * shell here.
 */
export type SessionTarget =
  | { kind: "profile"; profileId: string }
  | { kind: "quick"; protocol: "ssh" | "telnet"; username: string; host: string; port: number }
  | { kind: "local" };

function isCommandError(e: unknown): e is CommandError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export const errorMessage = (e: unknown): string => (isCommandError(e) ? e.message : String(e));

export const errorCode = (e: unknown): ErrorCode | null => (isCommandError(e) ? e.code : null);

export interface Session {
  id: SessionId;
  write(data: string): Promise<void>;
  resize(cols: number, rows: number): Promise<void>;
  /** Reports output bytes the terminal has processed (flow control). */
  ack(bytes: number): Promise<void>;
  close(): Promise<void>;
}

export const proxies = {
  list: () => invoke<Proxy[]>("proxies_list"),
  /** `password`: undefined keeps the stored password, "" clears it. */
  save: (proxy: Proxy, password?: string) => invoke<Saved<Proxy>>("proxy_save", { proxy, password: password ?? null }),
  /** Fails with `proxy.inUse` while a session uses it. */
  delete: (id: string) => invoke<void>("proxy_delete", { id }),
};

export const knownHosts = {
  /** `~/.ssh/known_hosts`, whether or not it exists; null without a home folder. */
  path: () => invoke<string | null>("known_hosts_path"),
  list: () => invoke<KnownHost[]>("known_hosts_list"),
  /** Lines of the entries for `host`, `host:port` or `[host]:port`, hashed ones included. */
  find: (query: string) => invoke<number[]>("known_hosts_find", { query }),
  /** Fails with `knownHosts.changed` if the line no longer reads `text`. */
  remove: (entry: KnownHost) => invoke<void>("known_hosts_remove", { line: entry.line, text: entry.text }),
};

export const listProfiles = () => invoke<Profile[]>("profiles_list");

/** `password`: undefined keeps the stored password, "" clears it. */
export const saveProfile = (profile: Profile, password?: string) =>
  invoke<Saved<Profile>>("profile_save", { profile, password: password ?? null });

export const deleteProfile = (id: string) => invoke<void>("profile_delete", { id });

/** Copies a profile (with its saved password) as `name`, right after it. */
export const duplicateProfile = (id: string, name: string) => invoke<Profile>("profile_duplicate", { id, name });

export const DEFAULT_GROUP = "default";

export const quickCommands = {
  get: () => invoke<QuickCommands>("quick_commands_get"),
  /** Stores all groups and commands; returns them as stored (new ones get ids). */
  set: (commands: QuickCommands) => invoke<QuickCommands>("quick_commands_set", { commands }),
};

export const tree = {
  folders: () => invoke<Folder[]>("folders_list"),
  /** Creates (empty id) or renames a folder. */
  saveFolder: (folder: Folder) => invoke<Folder>("folder_save", { folder }),
  /** Its sessions and subfolders move up into its parent. */
  deleteFolder: (id: string) => invoke<void>("folder_delete", { id }),
  /** Into `parent` (null: top level), before `before` (same kind) or at the end. */
  move: (item: TreeItem, parent: string | null, before: string | null) => invoke<void>("tree_move", { item, parent, before }),
};

export const sessionsFile = {
  export: (path: string) => invoke<void>("sessions_export", { path }),
  scan: (path: string) => invoke<SessionCandidate[]>("sessions_import_scan", { path }),
  import: (path: string, ids: string[]) => invoke<void>("sessions_import", { path, ids }),
};

/** The user's home folder, which `~` stands for in key paths. */
export const homeDirectory = () => invoke<string | null>("home_directory");
/** The folder the sessions and settings are saved in; created if needed. */
export const configDirectory = () => invoke<string>("config_directory");

/** The data files set aside at startup; returned once. */
export const configSetAside = () => invoke<SetAsideFile[]>("config_set_aside");

/** The user name `ssh` uses when none is given. */
export const localUsername = () => invoke<string>("local_username");

/** Replaces the profile's forwarding rules; resolves to the updated profile (with rule ids). */
export const setProfileForwards = (profileId: string, forwards: ForwardRule[]) =>
  invoke<Profile>("profile_set_forwards", { profileId, forwards });

export const sshConfig = {
  /** `~/.ssh/config`, whether or not it exists. */
  defaultPath: () => invoke<string | null>("ssh_config_default_path"),
  scan: (path: string) => invoke<ImportCandidate[]>("ssh_config_scan", { path }),
  /** Imports the hosts (and the jump hosts they need); resolves to the new profiles. */
  import: (path: string, aliases: string[]) => invoke<Profile[]>("ssh_config_import", { path, aliases }),
};

/**
 * A saved session's rules run once, on one of its connections, whichever tab acts on them;
 * their states arrive as `forward` session events in every tab of the session.
 */
export const forwards = {
  /** Starts the rule, or restarts it with this definition where it is running. */
  start: (id: SessionId, rule: ForwardRule) => invoke<void>("forward_start", { id, rule }),
  /** Stops the rule wherever it runs. */
  stop: (id: SessionId, ruleId: string) => invoke<void>("forward_stop", { id, ruleId }),
  /** The rules closing these sessions would stop although another tab of their session could keep them running. */
  keepCandidates: (ids: SessionId[]) => invoke<ForwardRule[]>("forward_keep_candidates", { ids }),
  /** Moves those rules to another tab of their session once the sessions close. */
  keep: (ids: SessionId[]) => invoke<void>("forward_keep", { ids }),
  /** The rules running on the session's connection, to start again on reconnecting (see `openSession`). */
  carry: (id: SessionId) => invoke<string[]>("forward_carry", { id }),
};

export const serialPorts = () => invoke<SerialPortInfo[]>("serial_ports");

/** Sends a break: on a serial line, or Telnet's BRK. */
export const sendBreak = (id: SessionId) => invoke<void>("session_break", { id });

/**
 * Starts a session for `target`. With `shareFrom` (SSH only), the shell runs on that
 * session's connection without connecting again; this fails with `session.notConnected` if
 * the connection is gone.
 */
export async function openSession(
  target: SessionTarget,
  size: { cols: number; rows: number },
  onOutput: (data: ArrayBuffer) => void,
  onEvent: (event: SessionEvent) => void,
  shareFrom?: SessionId,
  log: LogOpen = { mode: "auto" },
  /** Forwarding rules to start besides the automatic ones (see `forwards.carry`). */
  carry: string[] = [],
): Promise<Session> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = onOutput;
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  let spec: SessionSpec;
  if (target.kind === "profile") spec = shareFrom !== undefined ? { kind: "shared", source: shareFrom } : { ...target, carry };
  else spec = target;
  const id = await invoke<SessionId>("session_open", { spec, ...size, log, onOutput: output, onEvent: events });
  return {
    id,
    write: (data) => invoke("session_write", { id, data }),
    resize: (cols, rows) => invoke("session_resize", { id, cols, rows }),
    ack: (bytes) => invoke("session_ack", { id, bytes }),
    close: () => invoke("session_close", { id }),
  };
}

export const sessionLog = {
  /** Starts logging in a new file; the session reports it with a `log` event. */
  start: (id: SessionId) => invoke<string>("session_log_start", { id }),
  stop: (id: SessionId) => invoke<void>("session_log_stop", { id }),
};

export const logs = {
  /** How many logs ZShell has written (and still exist), and their total size. */
  summary: () => invoke<LogSummary>("logs_summary"),
  /** Where new logs go; created if needed. */
  directory: () => invoke<string>("logs_directory"),
  count: (profileId: string) => invoke<number>("logs_count", { profileId }),
  /** Deletes a session's logs, or all of them; those being written are kept. Returns how many were deleted. */
  delete: (profileId?: string) => invoke<number>("logs_delete", { profileId }),
};

/** Writes to a session as if typed in its terminal (the compose bar, quick commands). */
export const writeSession = (id: SessionId, data: string) => invoke<void>("session_write", { id, data });

/**
 * The program running in a local terminal other than its shell ("" if its name is unknown),
 * or null. Always null for SSH sessions.
 */
export const sessionForeground = (id: SessionId) => invoke<string | null>("session_foreground", { id });

export const zmodem = {
  /** `dir` null saves into the Downloads folder. */
  saveTo: (id: SessionId, dir: string | null) => invoke<void>("zmodem_save_to", { id, dir }),
  sendFiles: (id: SessionId, paths: string[]) => invoke<void>("zmodem_send_files", { id, paths }),
  /** Cancels the transfer, also while it waits for an answer. */
  cancel: (id: SessionId) => invoke<void>("zmodem_cancel", { id }),
};

/** Short name of the default local shell, e.g. "zsh" or "pwsh"; null when the system doesn't
 *  allow local terminals (Windows in S mode). */
export const localShellName = () => invoke<string | null>("local_shell_name");

function progressChannel(onProgress: (p: TransferProgress) => void) {
  const channel = new Channel<TransferProgress>();
  channel.onmessage = onProgress;
  return channel;
}

export const sftp = {
  /** Opens the session's SFTP channel; resolves to the remote home directory. */
  open: (id: SessionId) => invoke<string>("sftp_open", { id }),
  list: (id: SessionId, path: string) => invoke<Listing>("sftp_list", { id, path }),
  mkdir: (id: SessionId, path: string) => invoke<void>("sftp_mkdir", { id, path }),
  rename: (id: SessionId, from: string, to: string) => invoke<void>("sftp_rename", { id, from, to }),
  remove: (id: SessionId, path: string) => invoke<void>("sftp_remove", { id, path }),
  chmod: (id: SessionId, path: string, mode: number) => invoke<void>("sftp_chmod", { id, path, mode }),
  upload: (
    id: SessionId,
    transferId: string,
    localPaths: string[],
    remoteDir: string,
    onProgress: (p: TransferProgress) => void,
  ) =>
    invoke<void>("sftp_upload", { id, transferId, localPaths, remoteDir, onProgress: progressChannel(onProgress) }),
  /** `localDir` null downloads into the Downloads folder. Resolves to the created local paths. */
  download: (
    id: SessionId,
    transferId: string,
    remotePaths: string[],
    localDir: string | null,
    onProgress: (p: TransferProgress) => void,
  ) =>
    invoke<string[]>("sftp_download", {
      id,
      transferId,
      remotePaths,
      localDir,
      onProgress: progressChannel(onProgress),
    }),
  /** Downloads one item to `localPath`, replacing what is there. */
  downloadAs: (
    id: SessionId,
    transferId: string,
    remotePath: string,
    localPath: string,
    onProgress: (p: TransferProgress) => void,
  ) =>
    invoke<string>("sftp_download_as", { id, transferId, remotePath, localPath, onProgress: progressChannel(onProgress) }),
  /** The folder downloads go to without asking. */
  downloadsDirectory: () => invoke<string>("downloads_directory"),
  cancel: (transferId: string) => invoke<void>("transfer_cancel", { transferId }),
  /**
   * Downloads a file into a temporary folder and opens it in the editor; `onChanged` is called
   * each time it is saved. `editId` also identifies the download for `cancel`.
   */
  editOpen: (
    id: SessionId,
    editId: string,
    remotePath: string,
    onProgress: (p: TransferProgress) => void,
    onChanged: () => void,
  ) => {
    const onEvent = new Channel<EditEvent>();
    onEvent.onmessage = onChanged;
    return invoke<string>("sftp_edit_open", { id, editId, remotePath, onProgress: progressChannel(onProgress), onEvent });
  },
  editReopen: (editId: string) => invoke<void>("sftp_edit_reopen", { editId }),
  /** Fails with `edit.conflict` unless `force` when the remote file changed meanwhile. */
  editUpload: (id: SessionId, editId: string, force: boolean) => invoke<void>("sftp_edit_upload", { id, editId, force }),
  editStop: (editId: string) => invoke<void>("sftp_edit_stop", { editId }),
  /**
   * Drags the items out of the window (call while the mouse button is down). Resolves when the
   * drag ends; items dropped on another application are then downloaded, reported through
   * `onEvent`.
   */
  dragOut: (id: SessionId, transferId: string, items: FileEntry[], onEvent: (event: DragOutEvent) => void) => {
    const channel = new Channel<DragOutEvent>();
    channel.onmessage = onEvent;
    return invoke<DragResult>("sftp_drag_out", {
      id,
      transferId,
      items: items.map(({ path, isDir }): DragItem => ({ path, isDir })),
      onEvent: channel,
    });
  },
};
