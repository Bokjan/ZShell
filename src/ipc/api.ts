import { Channel } from "@tauri-apps/api/core";

import { call } from "./call";
import { listenHere } from "./events";

import type {
  AppLockSettings,
  CommandError,
  DragItem,
  DragOutEvent,
  EditEvent,
  ErrorCode,
  FileEntry,
  Folder,
  ForwardRule,
  NativeMenuSection,
  KnownHost,
  LogOpen,
  Profile,
  ProtectionChange,
  Proxy,
  QuickCommands,
  SessionEvent,
  SessionSnapshot,
  SessionSpec,
  Settings,
  TransferProgress,
  TreeItem,
} from "./bindings";

export type * from "./bindings";
export {
  ACK_BATCH,
  CLIPBOARD_LIMIT_MAX,
  DEFAULT_GROUP,
  DEFAULT_SERIAL,
  DEFAULT_SETTINGS,
  DEFAULT_TERM_TYPE,
  ENCODINGS,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  FORWARD_CHANNELS_LISTED,
  SCROLLBACK_MAX,
} from "./bindings";

/** The usual port of each network protocol. */
export const DEFAULT_PORTS = { ssh: 22, telnet: 23 } as const;

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

/** The error for an operation that finds the tab without a connection (`message` localized). */
export const notConnected = (message: string): CommandError => ({ code: "session.notConnected", params: {}, message });

export interface Session {
  id: SessionId;
  write(data: string): Promise<void>;
  /** Bytes that are not text, passed on without converting them to the session's encoding. */
  writeBytes(data: Uint8Array): Promise<void>;
  resize(cols: number, rows: number): Promise<void>;
  /** Reports output bytes the terminal has processed (flow control). */
  ack(bytes: number): Promise<void>;
  close(): Promise<void>;
}

export const proxies = {
  list: () => call("proxies_list"),
  /** `password`: undefined keeps the saved password, "" deletes it. */
  save: (proxy: Proxy, password?: string) => call("proxy_save", { proxy, password: password ?? null }),
  /** With its password. Fails with `proxy.inUse` while a session uses it. */
  delete: (id: string) => call("proxy_delete", { id }),
};

export const knownHosts = {
  /** `~/.ssh/known_hosts`, whether or not it exists; null without a home folder. */
  path: () => call("known_hosts_path"),
  list: () => call("known_hosts_list"),
  /** Lines of the entries for `host`, `host:port` or `[host]:port`, hashed ones included. */
  find: (query: string) => call("known_hosts_find", { query }),
  /** Fails with `knownHosts.changed` if the line no longer reads `text`. */
  remove: (entry: KnownHost) => call("known_hosts_remove", { line: entry.line, text: entry.text }),
};

export const listProfiles = () => call("profiles_list");

/** `password`: undefined keeps the saved password, "" deletes it. */
export const saveProfile = (profile: Profile, password?: string) => call("profile_save", { profile, password: password ?? null });

/** With its saved password. */
export const deleteProfile = (id: string) => call("profile_delete", { id });

/** Copies a profile (with its saved password) as `name`, right after it. */
export const duplicateProfile = (id: string, name: string) => call("profile_duplicate", { id, name });

export const quickCommands = {
  get: () => call("quick_commands_get"),
  /** Stores all groups and commands; returns them as stored (new ones get ids). */
  set: (commands: QuickCommands) => call("quick_commands_set", { commands }),
};

export const tree = {
  folders: () => call("folders_list"),
  /** The order the sessions and folders are shown in, mixed in each folder. */
  order: () => call("tree_order"),
  /** Creates (empty id) or renames a folder. */
  saveFolder: (folder: Folder) => call("folder_save", { folder }),
  /** Its sessions and subfolders move up into its parent. */
  deleteFolder: (id: string) => call("folder_delete", { id }),
  /** Into `parent` (null: top level), before `before` (a session or folder) or at the end. */
  move: (item: TreeItem, parent: string | null, before: TreeItem | null) => call("tree_move", { item, parent, before }),
};

export const sessionsFile = {
  /** With `passphrase`, the saved passwords come along, and the whole file is encrypted with it. */
  export: (path: string, passphrase?: string) => call("sessions_export", { path, passphrase: passphrase ?? null }),
  /**
   * An encrypted file takes its `passphrase`: fails with `import.passphraseRequired` without
   * one, `import.wrongPassphrase` with another one.
   */
  scan: (path: string, passphrase?: string) => call("sessions_import_scan", { path, passphrase: passphrase ?? null }),
  /** `digest` is the scan's: a file that changed since isn't imported. The passwords in the file come along. */
  import: (path: string, ids: string[], digest: string, passphrase?: string) =>
    call("sessions_import", { path, ids, digest, passphrase: passphrase ?? null }),
};

/** Where the sessions and passwords are kept, and how they are protected (see `vault/`). */
export const vault = {
  status: () => call("vault_status"),
  /** Unlocks with the key in the credential store, or makes the vault if there is none, reading the file again first. */
  tryUnlock: () => call("vault_try_unlock"),
  /** Fails with `vault.wrongPassword`. */
  unlock: (password: string) => call("vault_unlock", { password }),
  /** Deletes the locked vault: every session and password in it. `tryUnlock` then makes a new one. */
  reset: () => call("vault_reset"),
  /** `current`: the master password, to leave or change password protection. */
  setProtection: (change: ProtectionChange, current: string | null) => call("vault_set_protection", { change, current }),
  /** Whether the system's credential store can be used; on Linux there may be none. */
  storeAvailable: () => call("credential_store_available"),
};

/** The app lock (see `applock/mod.rs`). */
export const appLock = {
  /**
   * While the app lock is off, `method` and `unavailable` are null unless `method` asks for
   * them: the system takes a moment to answer (Windows Hello, polkit).
   */
  status: (method = false) => call("app_lock_status", { method }),
  /** Locks now, if the app lock is on. */
  lock: () => call("app_lock_lock"),
  /**
   * Asks the user to authenticate; fails with `appLock.cancelled` if they don't, and with
   * `appLock.gone` if there is no way to left. `anyway` then unlocks without.
   */
  unlock: (anyway = false) => call("app_lock_unlock", { anyway }),
  /** Turning the lock on asks the user to authenticate first. */
  configure: (settings: AppLockSettings) => call("app_lock_configure", { settings }),
};

/** The user's home folder, which `~` stands for in key paths. */
export const homeDirectory = () => call("home_directory");

/** Plays the system's alert sound (a terminal's bell). */
export const playBell = () => call("app_bell");
/**
 * The key file a session set to use one starts with: the first of OpenSSH's default keys
 * (`~/.ssh/id_ed25519` and so on) that exists here; null if there is none.
 */
export const defaultKeyPath = () => call("ssh_default_key_path");
/** Whether the key file at `path` (`~/…` included) exists here. */
export const keyFileExists = (path: string) => call("ssh_key_file_exists", { path });
/** The folder the sessions and settings are saved in; created if needed. */
export const configDirectory = () => call("config_directory");

/** The data files set aside at startup; returned once. */
export const configSetAside = () => call("config_set_aside");

/** The user name `ssh` uses when none is given. */
export const localUsername = () => call("local_username");

/** A session's port forwarding rules; only SSH sessions have them. */
export const profileForwards = (profile: Profile): ForwardRule[] =>
  profile.connection.protocol === "ssh" ? profile.connection.forwards : [];

/** Saving and deleting an SSH session's forwarding rules, one at a time. */
export const profileForwardRules = {
  /** Saves `rule` in place of the one with its id, or as a new one; resolves to the session and the rule as saved (with its id). */
  save: (profileId: string, rule: ForwardRule) => call("profile_forward_save", { profileId, rule }),
  /** Deletes the rule, which stops if it runs; resolves to the session. */
  delete: (profileId: string, ruleId: string) => call("profile_forward_delete", { profileId, ruleId }),
};

export const sshConfig = {
  /** `~/.ssh/config`, whether or not it exists. */
  defaultPath: () => call("ssh_config_default_path"),
  scan: (path: string) => call("ssh_config_scan", { path }),
  /** Imports the hosts (and the jump hosts they need); resolves to the new profiles. */
  import: (path: string, aliases: string[]) => call("ssh_config_import", { path, aliases }),
};

/**
 * A saved session's rules run once, on one of its connections, whichever tab acts on them;
 * their states arrive as `forward` session events in every tab of the session.
 */
export const forwards = {
  /** Starts the rule, or restarts it with this definition where it is running. */
  start: (id: SessionId, rule: ForwardRule) => call("forward_start", { id, rule }),
  /** Stops the rule wherever it runs. */
  stop: (id: SessionId, ruleId: string) => call("forward_stop", { id, ruleId }),
  /** The rules closing these sessions would stop although another tab of their session could keep them running. */
  keepCandidates: (ids: SessionId[]) => call("forward_keep_candidates", { ids }),
  /** How many connections the rules on these sessions' connections carry, which closing them cuts. */
  connections: (ids: SessionId[]) => call("forward_connections", { ids }),
  /** The sessions, in any window, whose connection runs the saved session's rules: deleting it stops them. */
  sessionsOf: (profileId: string) => call("forward_sessions_of", { profileId }),
  /** Moves those rules to another tab of their session once the sessions close. */
  keep: (ids: SessionId[]) => call("forward_keep", { ids }),
  /** The rules running on the session's connection, to start again on reconnecting (see `openSession`). */
  carry: (id: SessionId) => call("forward_carry", { id }),
};

export const serialPorts = () => call("serial_ports");

/** Lets the user pick a color from anywhere on the screen: `#rrggbb`, or `null` if they cancelled. */
export const pickColor = () => call("color_pick");

/** Sends a break: on a serial line, or Telnet's BRK. */
export const sendBreak = (id: SessionId) => call("session_break", { id });

/**
 * Starts a session for `target`. With `shareFrom` (SSH only), the shell runs on that
 * session's connection without connecting again; this fails with `session.notConnected` if
 * the connection is gone.
 */
export async function openSession(
  target: SessionTarget,
  size: { cols: number; rows: number },
  onOutput: (data: Uint8Array) => void,
  onEvent: (event: SessionEvent) => void,
  shareFrom?: SessionId,
  log: LogOpen = { mode: "auto" },
  /** Forwarding rules to start besides the automatic ones (see `forwards.carry`). */
  carry: string[] = [],
): Promise<Session> {
  let spec: SessionSpec;
  if (shareFrom !== undefined) spec = { kind: "shared", source: shareFrom };
  else if (target.kind === "profile") spec = { ...target, carry };
  else spec = target;
  const id = await call("session_open", { spec, ...size, log, channel: sessionChannel(onOutput, onEvent) });
  return sessionHandle(id);
}

/**
 * A session's output and events share one channel, which keeps them in order: each message is
 * a type byte (0 output, 1 event as JSON) and its content.
 */
function sessionChannel(onOutput: (data: Uint8Array) => void, onEvent: (event: SessionEvent) => void) {
  const channel = new Channel<ArrayBuffer>();
  const decoder = new TextDecoder();
  channel.onmessage = (message) => {
    const bytes = new Uint8Array(message);
    if (bytes[0] === 0) onOutput(bytes.subarray(1));
    else if (bytes[0] === 1) onEvent(JSON.parse(decoder.decode(bytes.subarray(1))) as SessionEvent);
  };
  return channel;
}

const sessionHandle = (id: SessionId): Session => ({
  id,
  write: (data) => call("session_write", { id, data }),
  writeBytes: (data) => call("session_write_bytes", { id, data: Array.from(data) }),
  resize: (cols, rows) => call("session_resize", { id, cols, rows }),
  ack: (bytes) => call("session_ack", { id, bytes }),
  close: () => call("session_close", { id }),
});

/** How long a session moving here waits for its old window's screen; it goes on without it. */
const SNAPSHOT_TIMEOUT_MS = 5000;

/** Who waits for the screen of each session moving to this window, by move. */
const snapshotWaiters = new Map<string, (snapshot: SessionSnapshot) => void>();
let snapshotListener: Promise<unknown> | undefined;

/**
 * Takes over a session that another window's tab showed (a tab moved here): its output and
 * events come here from now on. `snapshot` is what its terminal showed there, which the other
 * window hands over once the session has moved (null if it doesn't in time); output that
 * comes meanwhile goes after it.
 */
export async function adoptSession(
  id: SessionId,
  onOutput: (data: Uint8Array) => void,
  onEvent: (event: SessionEvent) => void,
): Promise<{ session: Session; snapshot: Promise<SessionSnapshot | null> }> {
  snapshotListener ??= listenHere("session-snapshot", ({ payload }) => snapshotWaiters.get(payload.moveId)?.(payload));
  await snapshotListener;
  // A screen handed over late for an earlier move of the session isn't this one's.
  const moveId = crypto.randomUUID();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const snapshot = new Promise<SessionSnapshot | null>((resolve) => {
    const done = (value: SessionSnapshot | null) => {
      clearTimeout(timer);
      snapshotWaiters.delete(moveId);
      resolve(value);
    };
    timer = setTimeout(() => done(null), SNAPSHOT_TIMEOUT_MS);
    snapshotWaiters.set(moveId, done);
  });
  try {
    await call("session_adopt", { id, moveId, channel: sessionChannel(onOutput, onEvent) });
  } catch (e) {
    clearTimeout(timer);
    snapshotWaiters.delete(moveId);
    throw e;
  }
  return { session: sessionHandle(id), snapshot };
}

/** Passes the screen of a session that moved away to the window it moved to. */
export const handOver = (snapshot: SessionSnapshot) => call("session_handover", { snapshot });

export const sessionLog = {
  /** Starts logging in a new file; the session reports it with a `log` event. */
  start: (id: SessionId) => call("session_log_start", { id }),
  stop: (id: SessionId) => call("session_log_stop", { id }),
};

export const logs = {
  /** How many logs ZShell has written (and still exist), and their total size. */
  summary: () => call("logs_summary"),
  /** Where new logs go; created only with `create`, which showing the folder needs. */
  directory: (create = false) => call("logs_directory", { create }),
  count: (profileId: string) => call("logs_count", { profileId }),
  /** Deletes a session's logs, or all of them; those being written are kept. Returns how many were deleted. */
  delete: (profileId?: string) => call("logs_delete", { profileId }),
};

/**
 * The program running in a local terminal other than its shell ("" if its name is unknown),
 * or null. Always null for SSH sessions.
 */
export const sessionForeground = (id: SessionId) => call("session_foreground", { id });

export const zmodem = {
  /** `dir` null saves into the Downloads folder. */
  saveTo: (id: SessionId, dir: string | null) => call("zmodem_save_to", { id, dir }),
  sendFiles: (id: SessionId, paths: string[]) => call("zmodem_send_files", { id, paths }),
  /** Cancels the transfer, also while it waits for an answer. */
  cancel: (id: SessionId) => call("zmodem_cancel", { id }),
};

/** Short name of the default local shell, e.g. "zsh" or "pwsh"; null when the system doesn't
 *  allow local terminals (Windows in S mode). */
export const localShellName = () => call("local_shell_name");

function progressChannel(onProgress: (p: TransferProgress) => void) {
  const channel = new Channel<TransferProgress>();
  channel.onmessage = onProgress;
  return channel;
}

export const sftp = {
  /** Opens the session's SFTP channel; resolves to the remote home directory. */
  open: (id: SessionId) => call("sftp_open", { id }),
  list: (id: SessionId, path: string) => call("sftp_list", { id, path }),
  /** The folder the session's shell is in, where the server lets it be found (Linux); null elsewhere. */
  shellDir: (id: SessionId) => call("sftp_shell_dir", { id }),
  mkdir: (id: SessionId, path: string) => call("sftp_mkdir", { id, path }),
  rename: (id: SessionId, from: string, to: string) => call("sftp_rename", { id, from, to }),
  remove: (id: SessionId, path: string) => call("sftp_remove", { id, path }),
  chmod: (id: SessionId, path: string, mode: number) => call("sftp_chmod", { id, path, mode }),
  upload: (id: SessionId, transferId: string, localPaths: string[], remoteDir: string, onProgress: (p: TransferProgress) => void) =>
    call("sftp_upload", { id, transferId, localPaths, remoteDir, onProgress: progressChannel(onProgress) }),
  /** `localDir` null downloads into the Downloads folder. Resolves to the created local paths. */
  download: (
    id: SessionId,
    transferId: string,
    remotePaths: string[],
    localDir: string | null,
    onProgress: (p: TransferProgress) => void,
  ) =>
    call("sftp_download", {
      id,
      transferId,
      remotePaths,
      localDir,
      onProgress: progressChannel(onProgress),
    }),
  /** Downloads one item to `localPath`, replacing what is there. */
  downloadAs: (id: SessionId, transferId: string, remotePath: string, localPath: string, onProgress: (p: TransferProgress) => void) =>
    call("sftp_download_as", { id, transferId, remotePath, localPath, onProgress: progressChannel(onProgress) }),
  /** The folder downloads go to without asking. */
  downloadsDirectory: () => call("downloads_directory"),
  cancel: (transferId: string) => call("transfer_cancel", { transferId }),
  /**
   * Downloads a file into a temporary folder and opens it in the editor; `onChanged` is called
   * each time it is saved. `editId` also identifies the download for `cancel`.
   */
  editOpen: (id: SessionId, editId: string, remotePath: string, onProgress: (p: TransferProgress) => void, onChanged: () => void) => {
    const onEvent = new Channel<EditEvent>();
    onEvent.onmessage = onChanged;
    return call("sftp_edit_open", { id, editId, remotePath, onProgress: progressChannel(onProgress), onEvent });
  },
  editReopen: (editId: string) => call("sftp_edit_reopen", { editId }),
  /** Fails with `edit.conflict` unless `force` when the remote file changed meanwhile. */
  editUpload: (id: SessionId, editId: string, force: boolean) => call("sftp_edit_upload", { id, editId, force }),
  editStop: (editId: string) => call("sftp_edit_stop", { editId }),
  /**
   * Drags the items out of the window (call while the mouse button is down, pressed at
   * `pressedAt`, an event's `timeStamp`). Resolves when the drag ends; items dropped on another
   * application are then downloaded, reported through `onEvent`.
   */
  dragOut: (id: SessionId, transferId: string, items: FileEntry[], pressedAt: number, onEvent: (event: DragOutEvent) => void) => {
    const channel = new Channel<DragOutEvent>();
    channel.onmessage = onEvent;
    return call("sftp_drag_out", {
      id,
      transferId,
      items: items.map(({ path, isDir, isSymlink, permissions, targetPermissions }): DragItem => ({
        path,
        isDir,
        permissions: isSymlink ? (targetPermissions ?? null) : permissions,
      })),
      sincePress: Math.max(0, Math.round(performance.now() - pressedAt)),
      onEvent: channel,
    });
  },
};

/** The app's settings, as the backend validated and saved them. */
export const appSettings = {
  get: () => call("settings_get"),
  set: (settings: Settings) => call("settings_set", { settings }),
  /** The desktop's monospace font (Linux), which terminals default to. */
  systemMonospaceFont: () => call("system_monospace_font"),
  /** Whether a screen reader is running now. */
  screenReaderRunning: () => call("screen_reader_running"),
  /** Whether the system lets the computer sleep on battery even while kept awake (Windows with Modern Standby). */
  keepAwakeLimited: () => call("keep_awake_limited"),
  /** The substitutions of the first installed of `families` (a font list, in order), or null. */
  ligatureRules: (families: string[]) => call("terminal_ligature_rules", { families }),
};

/** The languages in effect: the backend's follows the `language` setting. */
export const locales = () => call("locale_get");

/** Checking for updates (see `update.rs`). */
export const updates = {
  status: () => call("update_status"),
  check: () => call("update_check"),
};

/** The native menu bar (macOS), built from the page's menu. */
export const nativeMenu = {
  set: (sections: NativeMenuSection[]) => call("app_menu_set", { sections }),
  setLabel: (id: string, label: string) => call("app_menu_set_label", { id, label }),
};
