import { Channel, invoke } from "@tauri-apps/api/core";

export type SessionId = number;

export interface Session {
  id: SessionId;
  write(data: string): Promise<void>;
  resize(cols: number, rows: number): Promise<void>;
  close(): Promise<void>;
}

export async function openLoopbackSession(onOutput: (data: ArrayBuffer) => void): Promise<Session> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = onOutput;
  const id = await invoke<SessionId>("session_open_loopback", { onOutput: channel });
  return sessionHandle(id);
}

function sessionHandle(id: SessionId): Session {
  return {
    id,
    write: (data) => invoke("session_write", { id, data }),
    resize: (cols, rows) => invoke("session_resize", { id, cols, rows }),
    close: () => invoke("session_close", { id }),
  };
}
