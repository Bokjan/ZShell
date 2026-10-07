import { Channel, invoke } from "@tauri-apps/api/core";

export type AuthMethod = { type: "password" } | { type: "publicKey"; keyPath: string } | { type: "agent" };

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
  /** Edited with `setProfileForwards`; `saveProfile` leaves them unchanged. */
  forwards: ForwardRule[];
}

export type ForwardState =
  | { type: "starting" }
  | { type: "active"; bound: string; connections: number; lastError: CommandError | null }
  | { type: "failed"; error: CommandError }
  | { type: "stopped" };

export type SessionEvent =
  | { type: "connected" }
  | { type: "closed"; error: string | null }
  | { type: "forward"; ruleId: string; state: ForwardState };

export type SessionId = number;

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
  close(): Promise<void>;
}

export const listProfiles = () => invoke<Profile[]>("profiles_list");

/** `password`: undefined keeps the stored password, "" clears it. */
export const saveProfile = (profile: Profile, password?: string) =>
  invoke<Profile>("profile_save", { profile, password: password ?? null });

export const deleteProfile = (id: string) => invoke<void>("profile_delete", { id });

/** Replaces the profile's forwarding rules; resolves to the updated profile (with rule ids). */
export const setProfileForwards = (profileId: string, forwards: ForwardRule[]) =>
  invoke<Profile>("profile_set_forwards", { profileId, forwards });

/** Rule states arrive as `forward` session events. */
export const forwards = {
  /** Starts the rule, or restarts it with this definition if it is running. */
  start: (id: SessionId, rule: ForwardRule) => invoke<void>("forward_start", { id, rule }),
  stop: (id: SessionId, ruleId: string) => invoke<void>("forward_stop", { id, ruleId }),
};

export async function openSshSession(
  profileId: string,
  size: { cols: number; rows: number },
  onOutput: (data: ArrayBuffer) => void,
  onEvent: (event: SessionEvent) => void,
): Promise<Session> {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = onOutput;
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  const id = await invoke<SessionId>("ssh_open", { profileId, ...size, onOutput: output, onEvent: events });
  return {
    id,
    write: (data) => invoke("session_write", { id, data }),
    resize: (cols, rows) => invoke("session_resize", { id, cols, rows }),
    close: () => invoke("session_close", { id }),
  };
}

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
