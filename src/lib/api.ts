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

export interface Profile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  /** Ids of the profiles to connect through, first hop first (ProxyJump). */
  jumpHosts: string[];
  /** Seconds between keepalive messages; 0 disables them. */
  keepaliveInterval: number;
  /** Reconnect automatically when an established connection is lost. */
  autoReconnect: boolean;
  /** Edited with `setProfileForwards`; `saveProfile` leaves them unchanged. */
  forwards: ForwardRule[];
  /** The folder it is in; absent at the top level. Changed with `tree.move`. */
  folder?: string;
}

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
  | { type: "zmodem"; phase: ZmodemPhase };

export type SessionId = number;

/**
 * What a terminal tab runs: a saved SSH session, a quick connection without one (automatic
 * authentication; an empty username is the local user's), or the default shell here.
 */
export type SessionTarget =
  | { kind: "ssh"; profileId: string }
  | { kind: "quick"; username: string; host: string; port: number }
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

export const listProfiles = () => invoke<Profile[]>("profiles_list");

/** `password`: undefined keeps the stored password, "" clears it. */
export const saveProfile = (profile: Profile, password?: string) =>
  invoke<Profile>("profile_save", { profile, password: password ?? null });

export const deleteProfile = (id: string) => invoke<void>("profile_delete", { id });

/** Copies a profile (with its saved password) as `name`, right after it. */
export const duplicateProfile = (id: string, name: string) => invoke<Profile>("profile_duplicate", { id, name });

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
  host: string;
  port: number;
  username: string;
  /** Folder names, outermost first. */
  folder: string[];
  jumpHosts: string[];
  /** Name of an existing session with the same name or address; not imported again. */
  existing: string | null;
}

export const sessionsFile = {
  export: (path: string) => invoke<void>("sessions_export", { path }),
  scan: (path: string) => invoke<SessionCandidate[]>("sessions_import_scan", { path }),
  import: (path: string, ids: string[]) => invoke<void>("sessions_import", { path, ids }),
};

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
  /** ProxyJump entries as written in the config. */
  jumpHosts: string[];
  keepaliveInterval: number;
  forwards: ForwardRule[];
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
): Promise<Session> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = onOutput;
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  const args = { ...size, onOutput: output, onEvent: events };
  let id: SessionId;
  if (target.kind === "local") id = await invoke<SessionId>("local_open", args);
  else if (target.kind === "quick") {
    const { username, host, port } = target;
    id = await invoke<SessionId>("ssh_quick_open", { username, host, port, ...args });
  } else if (shareFrom !== undefined) id = await invoke<SessionId>("ssh_open_shared", { source: shareFrom, ...args });
  else id = await invoke<SessionId>("ssh_open", { profileId: target.profileId, ...args });
  return {
    id,
    write: (data) => invoke("session_write", { id, data }),
    resize: (cols, rows) => invoke("session_resize", { id, cols, rows }),
    ack: (bytes) => invoke("session_ack", { id, bytes }),
    close: () => invoke("session_close", { id }),
  };
}

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

/** Short name of the default local shell, e.g. "zsh" or "pwsh". */
export const localShellName = () => invoke<string>("local_shell_name");

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
  cancel: (transferId: string) => invoke<void>("transfer_cancel", { transferId }),
};
