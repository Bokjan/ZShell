import type { ChannelInfo, ConnectionInfo, Counts, ForwardTraffic, SftpChannelTraffic } from "../ipc/api";

/** How far back a rate looks: long enough to read steadily, short enough to follow a change. */
export const RATE_WINDOW_MS = 1000;

/** Nothing counted. */
export const NO_COUNTS: Counts = { sent: 0, received: 0, lastSent: null, lastReceived: null };

/** Bytes a second, each way. */
export interface Rate {
  sent: number;
  received: number;
}

/** What the lights and speeds read: a meter, or several together. */
export interface TrafficSource {
  readonly counts: Counts;
  rate(): Rate;
  /** Calls `listener` on each change, which can be many a second: it should only schedule work. */
  subscribe(listener: () => void): () => void;
}

class Listeners {
  private readonly listeners = new Set<() => void>();

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => void this.listeners.delete(listener);
  }

  notify() {
    for (const listener of this.listeners) listener();
  }
}

/**
 * What a channel (or a set of them) has carried, as the backend last reported it (see
 * `traffic.rs`), with what it is about (`detail`: its channel and connection), and the reports
 * of the last few seconds, for its rate.
 */
export class TrafficMeter<D> implements TrafficSource {
  counts: Counts = NO_COUNTS;
  detail: D | null = null;
  private samples: { at: number; sent: number; received: number }[] = [];
  private readonly listeners = new Listeners();

  constructor(private readonly now: () => number = Date.now) {}

  update(counts: Counts, detail: D) {
    this.counts = counts;
    this.detail = detail;
    const at = this.now();
    this.samples.push({ at, sent: counts.sent, received: counts.received });
    // The last one before the window stays, as where a rate starts.
    const inside = this.samples.findIndex((sample) => sample.at > at - RATE_WINDOW_MS);
    if (inside > 1) this.samples.splice(0, inside - 1);
    this.notify();
  }

  /** A new connection: nothing counted yet. */
  reset() {
    this.counts = NO_COUNTS;
    this.detail = null;
    this.samples = [];
    this.notify();
  }

  /**
   * What it carried a second over the last `RATE_WINDOW_MS`: 0 once reports have stopped for
   * that long. The counts at the window's start are those of the last report before it (nothing
   * came between that report and the next), or of the first report if none came before.
   */
  rate(): Rate {
    const last = this.samples[this.samples.length - 1];
    if (!last) return { sent: 0, received: 0 };
    const now = this.now();
    let from = this.samples[0];
    for (const sample of this.samples) {
      if (sample.at > now - RATE_WINDOW_MS) break;
      from = sample;
    }
    const seconds = RATE_WINDOW_MS / 1000;
    return { sent: (last.sent - from.sent) / seconds, received: (last.received - from.received) / seconds };
  }

  subscribe(listener: () => void): () => void {
    return this.listeners.subscribe(listener);
  }

  private notify() {
    this.listeners.notify();
  }
}

/** What an SFTP channel's meter is about: the connection, and the channel once it has opened. */
export interface SftpDetail {
  connection: ConnectionInfo;
  channel: number | null;
}

/** What a forwarding rule's meter is about. */
export type ForwardDetail = Omit<ForwardTraffic, "counts">;

/** What the SFTP channels of a pane's connection carry: browsing, and transfers. */
export class SftpTraffic {
  readonly browse: TrafficMeter<SftpDetail>;
  readonly transfer: TrafficMeter<SftpDetail>;

  constructor(now: () => number = Date.now) {
    this.browse = new TrafficMeter(now);
    this.transfer = new TrafficMeter(now);
  }

  update(connection: ConnectionInfo, browse: SftpChannelTraffic, transfer: SftpChannelTraffic) {
    this.browse.update(browse.counts, { connection, channel: browse.channel });
    this.transfer.update(transfer.counts, { connection, channel: transfer.channel });
  }

  reset() {
    this.browse.reset();
    this.transfer.reset();
  }
}

/** The meters of a session's forwarding rules that run, by rule id; also all of them together. */
export class ForwardMeters implements TrafficSource {
  private readonly meters = new Map<string, TrafficMeter<ForwardDetail>>();
  private readonly listeners = new Listeners();

  constructor(private readonly now: () => number = Date.now) {}

  get(ruleId: string): TrafficMeter<ForwardDetail> | undefined {
    return this.meters.get(ruleId);
  }

  all(): TrafficMeter<ForwardDetail>[] {
    return [...this.meters.values()];
  }

  update(ruleId: string, { counts, ...detail }: ForwardTraffic) {
    let meter = this.meters.get(ruleId);
    if (!meter) this.meters.set(ruleId, (meter = new TrafficMeter(this.now)));
    meter.update(counts, detail);
    this.listeners.notify();
  }

  /** The rule stopped, or starts again: a new run counts afresh. */
  clear(ruleId: string) {
    if (this.meters.delete(ruleId)) this.listeners.notify();
  }

  reset() {
    this.meters.clear();
    this.listeners.notify();
  }

  get counts(): Counts {
    const latest = (a: number | null, b: number | null) => (a === null ? b : b === null ? a : Math.max(a, b));
    return this.all().reduce(
      (sum, { counts }) => ({
        sent: sum.sent + counts.sent,
        received: sum.received + counts.received,
        lastSent: latest(sum.lastSent, counts.lastSent),
        lastReceived: latest(sum.lastReceived, counts.lastReceived),
      }),
      NO_COUNTS,
    );
  }

  rate(): Rate {
    return this.all().reduce(
      (sum, meter) => {
        const rate = meter.rate();
        return { sent: sum.sent + rate.sent, received: sum.received + rate.received };
      },
      { sent: 0, received: 0 },
    );
  }

  /** On a change of any rule's meter, or of which rules have one. */
  subscribe(listener: () => void): () => void {
    return this.listeners.subscribe(listener);
  }
}

/**
 * Channel numbers as they are listed: runs of three or more as one range ("7–9"), lowest
 * first.
 */
export function channelRanges(channels: readonly number[]): string[] {
  const sorted = [...new Set(channels)].sort((a, b) => a - b);
  const ranges: string[] = [];
  for (let i = 0; i < sorted.length;) {
    let end = i;
    while (end + 1 < sorted.length && sorted[end + 1] === sorted[end] + 1) end++;
    if (end - i >= 2) ranges.push(`${sorted[i]}–${sorted[end]}`);
    else for (let j = i; j <= end; j++) ranges.push(String(sorted[j]));
    i = end + 1;
  }
  return ranges;
}

/** The terminal's channel, for its meter's detail. */
export type TerminalDetail = ChannelInfo | null;
