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
