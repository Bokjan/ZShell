/** The number of bytes `text` takes in UTF-8, without encoding it. */
export function utf8Length(text: string): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    // A surrogate pair is one character of 4 bytes; a lone surrogate is encoded as U+FFFD.
    else if (code >= 0xd800 && code < 0xdc00 && i + 1 < text.length && (text.charCodeAt(i + 1) & 0xfc00) === 0xdc00) {
      bytes += 4;
      i++;
    } else bytes += 3;
  }
  return bytes;
}

/**
 * What a pane's terminal has sent and received on its current connection, for the status
 * bar: terminal data only, as the terminal has it (after the session's encoding, without the
 * protocol's own traffic such as SSH keepalives, file transfers and port forwarding).
 */
export class TrafficMeter {
  sent = 0;
  received = 0;
  /** When data was last sent and received (`Date.now()`), if it has been. */
  lastSent: number | null = null;
  lastReceived: number | null = null;
  private readonly listeners = new Set<() => void>();

  constructor(private readonly now: () => number = Date.now) {}

  /** A new connection: counted afresh. */
  reset() {
    this.sent = this.received = 0;
    this.lastSent = this.lastReceived = null;
    this.notify();
  }

  addSent(bytes: number) {
    this.sent += bytes;
    this.lastSent = this.now();
    this.notify();
  }

  addReceived(bytes: number) {
    this.received += bytes;
    this.lastReceived = this.now();
    this.notify();
  }

  /** Calls `listener` on each change, which can be many a second: it should only schedule work. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  }

  private notify() {
    for (const listener of this.listeners) listener();
  }
}
