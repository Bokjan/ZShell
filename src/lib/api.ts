import { Channel, invoke } from "@tauri-apps/api/core";

export type AuthMethod =
  | { type: "auto" }
  | { type: "password" }
  | { type: "publicKey"; keyPath: string }
  | { type: "agent" };

export type ForwardKind = "local" | "remote" | "dynamic";

/** A saved port forwarding rule; `target*` is unused for dynamic (SOCKS) rules. */
export interface ForwardRule {
  /** Empty for a new rule; assigned on save. */
  id: string;
  kind: ForwardKind;
  bindHost: string;
  /** 0 picks a free port. */
  bindPort: number;
  targetHost: string;
  targetPort: number;
  description: string;
  autoStart: boolean;
}

export type Protocol = "ssh" | "telnet" | "serial";

export type Parity = "none" | "odd" | "even";
export type FlowControl = "none" | "software" | "hardware";

/** A serial line's device and settings. */
export interface SerialOptions {
  /** `/dev/cu.*` on macOS, `COM3` on Windows. */
  device: string;
  baudRate: number;
  /** 5 to 8. */
  dataBits: number;
  parity: Parity;
  /** 1 or 2. */
  stopBits: number;
  flowControl: FlowControl;
}

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

export interface Profile {
  id: string;
  name: string;
  protocol: Protocol;
  /** SSH and Telnet. */
  host: string;
  port: number;
  /** Required for SSH; for Telnet, typed at the login prompt if set. */
  username: string;
  /** SSH only. */
  auth: AuthMethod;
  /** Serial sessions; absent when all settings are the defaults (`DEFAULT_SERIAL`, no device). */
  serial?: SerialOptions;
  /** Ids of the profiles to connect through, first hop first (ProxyJump). */
  jumpHosts: string[];
  /** The id of the proxy to connect through (SSH and Telnet); absent without one, and with jump hosts, where the first jump host's own proxy is used. */
  proxy?: string;
  /** Seconds between keepalive messages; 0 disables them. */
  keepaliveInterval: number;
  /** Reconnect automatically when an established connection is lost. */
  autoReconnect: boolean;
  /** Edited with `setProfileForwards`; `saveProfile` leaves them unchanged. */
  forwards: ForwardRule[];
  /** The folder it is in; absent at the top level. Changed with `tree.move`. */
  folder?: string;
  /** The quick command group its tabs show first; absent for the default group. */
  commandGroup?: string;
  /** Record a session log from the start of each connection. */
  autoLog: boolean;
  /** Let the remote shell use the local SSH agent (ForwardAgent). */
  forwardAgent: boolean;
  /** The remote side's character encoding, one of `ENCODINGS`. */
  encoding: string;
  /** The terminal type the remote shell is told (TERM). */
  termType: string;
  /** Environment variables for the remote shell (SetEnv). */
  env: EnvVar[];
  /** Typed into each new shell, in order, each once the shell shows a prompt. */
  loginCommands: string[];
  /** Terminal appearance for this session; absent values follow the settings. */
  appearance?: ProfileAppearance;
}

export interface EnvVar {
  name: string;
  value: string;
}

export interface ProfileAppearance {
  /** A color scheme id, or "auto". */
  colorScheme?: string;
  /** Replaces the scheme's background, as #rrggbb. */
  background?: string;
  fontFamily?: string;
  fontSize?: number;
}

/** The character encodings a session can use (WHATWG labels, as the backend stores them). */
export const ENCODINGS = ["utf-8", "gb18030", "gbk", "big5", "shift_jis", "euc-jp", "euc-kr", "windows-1252"] as const;

export const DEFAULT_TERM_TYPE = "xterm-256color";

export interface Folder {
  /** Empty for a new folder; assigned on save. */
  id: string;
  name: string;
  /** Absent at the top level. */
  parent?: string;
}

/** A session or folder, as moved in the sidebar. */
export type TreeItem = { kind: "profile"; id: string } | { kind: "folder"; id: string };

export type ForwardState =
  | { type: "starting" }
  | { type: "active"; bound: string; connections: number; lastError: CommandError | null }
  | { type: "failed"; error: CommandError }
  | { type: "stopped" };

/** exited: the shell ended; lost: an established connection broke; failed: never got connected. */
export type CloseReason = "exited" | "lost" | "failed";

/**
 * A ZMODEM transfer in the terminal: `sz` asks where to save (answer with `zmodem.saveTo`),
 * `rz` asks for files (`zmodem.sendFiles`), then it runs until "idle".
 */
export type ZmodemPhase = "chooseDestination" | "chooseFiles" | "transferring" | "idle";

export type SessionEvent =
  | { type: "connected" }
  /** `status`: the shell's exit status, if it reported one. */
  | { type: "closed"; reason: CloseReason; error: CommandError | null; status: number | null }
  | { type: "forward"; ruleId: string; state: ForwardState }
  | { type: "zmodem"; phase: ZmodemPhase }
  /** The session's log started (`path`), stopped (neither), or couldn't start (`error`). */
  | { type: "log"; path: string | null; error: CommandError | null };

/** How a new session's log starts: as the session or settings say, on with a file after reconnecting, or not at all. */
export type LogOpen = { mode: "auto" } | { mode: "append"; path: string } | { mode: "off" };

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

/** Error returned by backend commands; `message` is already localized by the backend. */
export interface CommandError {
  code: string;
  params: Record<string, string>;
  message: string;
}

function isCommandError(e: unknown): e is CommandError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export const errorMessage = (e: unknown): string => (isCommandError(e) ? e.message : String(e));

export const errorCode = (e: unknown): string | null => (isCommandError(e) ? e.code : null);

export interface Session {
  id: SessionId;
  write(data: string): Promise<void>;
  resize(cols: number, rows: number): Promise<void>;
  /** Reports output bytes the terminal has processed (flow control). */
  ack(bytes: number): Promise<void>;
  close(): Promise<void>;
}

export type ProxyKind = "socks5" | "http" | "command";

/** A saved proxy for the first connection of a session. Fields of other kinds are kept. */
export interface Proxy {
  /** Empty for a new proxy; assigned on save. */
  id: string;
  /** Named after the address or program when saved empty. */
  name: string;
  kind: ProxyKind;
  /** SOCKS5 and HTTP. */
  host: string;
  port: number;
  /** SOCKS5 and HTTP; empty without authentication. The password is in the keychain. */
  username: string;
  /** Command proxies: run with %h, %p, %r and %% replaced (ProxyCommand). */
  command: string;
}

export const proxies = {
  list: () => invoke<Proxy[]>("proxies_list"),
  /** `password`: undefined keeps the stored password, "" clears it. */
  save: (proxy: Proxy, password?: string) => invoke<Proxy>("proxy_save", { proxy, password: password ?? null }),
  /** Fails with `proxy.inUse` while a session uses it. */
  delete: (id: string) => invoke<void>("proxy_delete", { id }),
};

/** A host key line of `~/.ssh/known_hosts`. */
export interface KnownHost {
  /** 1-based, counting every line of the file. */
  line: number;
  /** The line as written; removing the entry checks it is still there. */
  text: string;
  /** `cert-authority` or `revoked`. */
  marker: string | null;
  /** Host names or patterns; hashed ones as written (`|1|salt|hash`). */
  hosts: string[];
  algorithm: string;
  /** `SHA256:…`; null when the key can't be read. */
  fingerprint: string | null;
  comment: string | null;
}

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
  invoke<Profile>("profile_save", { profile, password: password ?? null });

export const deleteProfile = (id: string) => invoke<void>("profile_delete", { id });

/** Copies a profile (with its saved password) as `name`, right after it. */
export const duplicateProfile = (id: string, name: string) => invoke<Profile>("profile_duplicate", { id, name });

export interface QuickCommand {
  /** Empty for a new command; assigned on save. */
  id: string;
  name: string;
  /** Sent as typed; line breaks are Enter. */
  text: string;
  /** Press Enter after the text; otherwise it is left on the command line to be finished. */
  enter: boolean;
}

export interface CommandGroup {
  /** `DEFAULT_GROUP` for the group that always exists; empty for a new one. */
  id: string;
  /** Empty for the default group, whose name is shown translated. */
  name: string;
  commands: QuickCommand[];
}

export interface QuickCommands {
  /** The default group first. */
  groups: CommandGroup[];
}

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

/** A session in an exported file, as it would be imported. */
export interface SessionCandidate {
  id: string;
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  serial: SerialOptions;
  /** Folder names, outermost first. */
  folder: string[];
  jumpHosts: string[];
  /** The name of its proxy. */
  proxy: string | null;
  /** The command of its proxy, for a command proxy (it runs when the session connects). */
  proxyCommand: string | null;
  /** Name of an existing session with the same name or address; not imported again. */
  existing: string | null;
}

export const sessionsFile = {
  export: (path: string) => invoke<void>("sessions_export", { path }),
  scan: (path: string) => invoke<SessionCandidate[]>("sessions_import_scan", { path }),
  import: (path: string, ids: string[]) => invoke<void>("sessions_import", { path, ids }),
};

/** The user's home folder, which `~` stands for in key paths. */
export const homeDirectory = () => invoke<string | null>("home_directory");

/** The user name `ssh` uses when none is given. */
export const localUsername = () => invoke<string>("local_username");

/** Replaces the profile's forwarding rules; resolves to the updated profile (with rule ids). */
export const setProfileForwards = (profileId: string, forwards: ForwardRule[]) =>
  invoke<Profile>("profile_set_forwards", { profileId, forwards });

/** A host from an OpenSSH client config, as it would be imported. */
export interface ImportCandidate {
  alias: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  /** ProxyJump entries as written in the config (also the host of `ProxyCommand ssh -W %h:%p host`). */
  jumpHosts: string[];
  /** Any other ProxyCommand, imported as a command proxy. */
  proxyCommand: string | null;
  keepaliveInterval: number;
  forwards: ForwardRule[];
  forwardAgent: boolean;
  /** From SetEnv. */
  env: EnvVar[];
  /** Name of an existing profile for the same host; such hosts are not imported again. */
  existing: string | null;
  /** Config options that are not imported. */
  skipped: string[];
}

export const sshConfig = {
  /** `~/.ssh/config`, whether or not it exists. */
  defaultPath: () => invoke<string | null>("ssh_config_default_path"),
  scan: (path: string) => invoke<ImportCandidate[]>("ssh_config_scan", { path }),
  /** Imports the hosts (and the jump hosts they need); resolves to the new profiles. */
  import: (path: string, aliases: string[]) => invoke<Profile[]>("ssh_config_import", { path, aliases }),
};

/** Rule states arrive as `forward` session events. */
export const forwards = {
  /** Starts the rule, or restarts it with this definition if it is running. */
  start: (id: SessionId, rule: ForwardRule) => invoke<void>("forward_start", { id, rule }),
  stop: (id: SessionId, ruleId: string) => invoke<void>("forward_stop", { id, ruleId }),
};

/** A serial port on this computer: its name (`/dev/cu.usbserial-1410`, `COM3`) and USB product. */
export interface SerialPortInfo {
  name: string;
  description: string | null;
}

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
): Promise<Session> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = onOutput;
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  const args = { ...size, log, onOutput: output, onEvent: events };
  let id: SessionId;
  if (target.kind === "local") id = await invoke<SessionId>("local_open", args);
  else if (target.kind === "quick") {
    const { protocol, username, host, port } = target;
    id = await invoke<SessionId>("quick_open", { protocol, username, host, port, ...args });
  } else if (shareFrom !== undefined) id = await invoke<SessionId>("ssh_open_shared", { source: shareFrom, ...args });
  else id = await invoke<SessionId>("profile_open", { profileId: target.profileId, ...args });
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
  summary: () => invoke<{ count: number; bytes: number }>("logs_summary"),
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

export interface FileEntry {
  name: string;
  path: string;
  /** True for directories and symlinks to directories. */
  isDir: boolean;
  isSymlink: boolean;
  size: number;
  /** Seconds since the Unix epoch. */
  modified: number | null;
  permissions: number | null;
}

export interface Listing {
  path: string;
  entries: FileEntry[];
}

export interface TransferProgress {
  transferred: number;
  total: number;
  filesDone: number;
  filesTotal: number;
  current: string;
}

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
    const onEvent = new Channel<{ type: "changed" }>();
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
      items: items.map(({ path, isDir }) => ({ path, isDir })),
      onEvent: channel,
    });
  },
};

/** How a drag out ended; `x` and `y` are where it was dropped in the page, for "inside". */
export interface DragResult {
  outcome: "cancelled" | "inside" | "outside";
  x: number;
  y: number;
}

/** A download for a drag out of the window, which starts once the items are dropped. */
export type DragOutEvent =
  | ({ type: "progress" } & TransferProgress)
  | { type: "done"; paths: string[] }
  | { type: "error"; error: unknown };
