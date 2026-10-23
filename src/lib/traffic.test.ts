import { describe, expect, it, vi } from "vitest";

import type { Counts } from "../ipc/api";
import { channelRanges, ForwardMeters, RATE_WINDOW_MS, TrafficMeter } from "./traffic";

const counts = (sent: number, received: number, at: number | null = null): Counts => ({ sent, received, lastSent: at, lastReceived: at });

describe("TrafficMeter", () => {
  it("keeps the latest report until reset", () => {
    const meter = new TrafficMeter<string>(() => 0);
    meter.update(counts(3, 150, 1000), "channel 2");
    expect(meter).toMatchObject({ counts: counts(3, 150, 1000), detail: "channel 2" });
    meter.reset();
    expect(meter).toMatchObject({ counts: counts(0, 0), detail: null });
  });

  it("tells its listeners of each change until they unsubscribe", () => {
    const meter = new TrafficMeter<null>();
    const listener = vi.fn();
    const unsubscribe = meter.subscribe(listener);
    meter.update(counts(1, 0), null);
    meter.reset();
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
    meter.update(counts(2, 0), null);
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("takes its rate over the last few seconds", () => {
    let time = 0;
    const meter = new TrafficMeter<null>(() => time);
    expect(meter.rate()).toEqual({ sent: 0, received: 0 });
    // 1000 bytes a second received, in reports every 50 ms.
    for (; time <= 5000; time += 50) meter.update(counts(10, time), null);
    time -= 50;
    expect(meter.rate().received).toBeCloseTo(1000);
    expect(meter.rate().sent).toBe(0);
    // Reports stop: the rate falls to nothing once the window has passed.
    time += RATE_WINDOW_MS / 2;
    expect(meter.rate().received).toBeCloseTo(500);
    time += RATE_WINDOW_MS / 2;
    expect(meter.rate().received).toBe(0);
  });

  it("follows traffic that starts after a long quiet spell at once", () => {
    let time = 0;
    const meter = new TrafficMeter<null>(() => time);
    meter.update(counts(0, 1000), null);
    time = 60_000;
    meter.update(counts(0, 1100), null);
    time = 60_050;
    meter.update(counts(0, 1200), null);
    expect(meter.rate().received).toBe(200 / (RATE_WINDOW_MS / 1000));
  });

  it("doesn't make a spike of the first report", () => {
    let time = 0;
    const meter = new TrafficMeter<null>(() => time);
    meter.update(counts(0, 0), null);
    time = 50;
    meter.update(counts(0, 100), null);
    expect(meter.rate().received).toBe(100);
  });
});

describe("ForwardMeters", () => {
  const detail = { connection: { id: 3, localPort: null }, channels: [7], channelCount: 1 };

  it("adds up the rules that run", () => {
    let time = 0;
    const meters = new ForwardMeters(() => time);
    const listener = vi.fn();
    meters.subscribe(listener);
    meters.update("a", { counts: counts(0, 0), ...detail });
    meters.update("b", { counts: counts(0, 0), ...detail });
    time = 2000;
    meters.update("a", { counts: { sent: 1000, received: 4000, lastSent: 1500, lastReceived: 1800 }, ...detail });
    meters.update("b", { counts: { sent: 2000, received: 0, lastSent: 1900, lastReceived: null }, ...detail });
    expect(meters.counts).toEqual({ sent: 3000, received: 4000, lastSent: 1900, lastReceived: 1800 });
    // Over the last second, which began after the first reports.
    expect(meters.rate()).toEqual({ sent: 3000, received: 4000 });
    expect(meters.rule("a").detail).toEqual(detail);

    meters.clear("b");
    expect(meters.running()).toHaveLength(1);
    // Only a rule that ran is told of.
    meters.clear("c");
    meters.reset();
    expect(meters.counts).toEqual(counts(0, 0));
    expect(listener).toHaveBeenCalledTimes(6);
  });

  it("keeps a rule's meter from run to run, so that what shows it follows the next run", () => {
    const meters = new ForwardMeters(() => 0);
    const meter = meters.rule("a");
    const listener = vi.fn();
    meter.subscribe(listener);
    meters.update("a", { counts: counts(5, 5), ...detail });
    meters.clear("a");
    expect(meter).toMatchObject({ counts: counts(0, 0), detail: null });
    meters.update("a", { counts: counts(1, 0), ...detail });
    expect(meters.rule("a")).toBe(meter);
    expect(meter.counts.sent).toBe(1);
    expect(listener).toHaveBeenCalledTimes(3);
  });
});

describe("channelRanges", () => {
  it("lists runs of three or more as ranges", () => {
    expect(channelRanges([])).toEqual([]);
    expect(channelRanges([15, 7, 8, 9, 11, 12, 14])).toEqual(["7–9", "11", "12", "14", "15"]);
    expect(channelRanges([3, 3, 4, 5])).toEqual(["3–5"]);
  });
});
