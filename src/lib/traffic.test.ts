import { describe, expect, it, vi } from "vitest";

import { TrafficMeter, utf8Length } from "./traffic";

describe("utf8Length", () => {
  it("counts the bytes of the UTF-8 encoding", () => {
    for (const text of ["", "ls -l\r", "café", "中文", "\u{1F600}", "a\u{1F600}b", "\ud800x", "x\udc00"]) {
      expect(utf8Length(text), text).toBe(new TextEncoder().encode(text).length);
    }
  });
});

describe("TrafficMeter", () => {
  it("adds up each direction with the time it last had data", () => {
    let time = 1000;
    const meter = new TrafficMeter(() => time);
    meter.addSent(3);
    time = 2000;
    meter.addReceived(100);
    meter.addReceived(50);
    expect(meter).toMatchObject({ sent: 3, received: 150, lastSent: 1000, lastReceived: 2000 });
    meter.reset();
    expect(meter).toMatchObject({ sent: 0, received: 0, lastSent: null, lastReceived: null });
  });

  it("tells its listeners of each change until they unsubscribe", () => {
    const meter = new TrafficMeter();
    const listener = vi.fn();
    const unsubscribe = meter.subscribe(listener);
    meter.addSent(1);
    meter.addReceived(1);
    meter.reset();
    expect(listener).toHaveBeenCalledTimes(3);
    unsubscribe();
    meter.addSent(1);
    expect(listener).toHaveBeenCalledTimes(3);
  });
});
