import { Channel, invoke } from "@tauri-apps/api/core";

export type AuthMethod = { type: "password" } | { type: "publicKey"; keyPath: string } | { type: "agent" };

export interface Profile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
}

export type SessionEvent = { type: "connected" } | { type: "closed"; error: string | null };

export type SessionId = number;

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
