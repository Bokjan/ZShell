import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Disposables, Timer } from "./disposables";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("Timer", () => {
  it("runs once its time has passed", () => {
    const timer = new Timer();
    const ran = vi.fn();
    timer.start(ran, 100);
    vi.advanceTimersByTime(99);
    expect(ran).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(ran).toHaveBeenCalledOnce();
  });

  it("starts over, running only the last", () => {
    const timer = new Timer();
    const first = vi.fn();
    const second = vi.fn();
    timer.start(first, 100);
    vi.advanceTimersByTime(50);
    timer.start(second, 100);
    vi.advanceTimersByTime(99);
    expect(second).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledOnce();
  });

  it("runs nothing once cleared or disposed", () => {
    const timer = new Timer();
    const ran = vi.fn();
    timer.start(ran, 100);
    timer.clear();
    timer.start(ran, 100);
    timer.dispose();
    vi.advanceTimersByTime(1000);
    expect(ran).not.toHaveBeenCalled();
  });
});

describe("Disposables", () => {
  it("lets go of what it holds, the last added first", () => {
    const held = new Disposables();
    const order: string[] = [];
    held.add({ dispose: () => order.push("first") });
    held.defer(() => order.push("second"));
    held.add({ dispose: () => order.push("third") });
    held.dispose();
    expect(order).toEqual(["third", "second", "first"]);
  });

  it("lets go of each once", () => {
    const held = new Disposables();
    const disposed = vi.fn();
    held.defer(disposed);
    held.dispose();
    held.dispose();
    expect(disposed).toHaveBeenCalledOnce();
  });

  it("lets go at once of what is added once disposed", () => {
    const held = new Disposables();
    held.dispose();
    const disposed = vi.fn();
    held.defer(disposed);
    expect(disposed).toHaveBeenCalledOnce();
  });

  it("stops listening, with the same capture", () => {
    const held = new Disposables();
    const target = new EventTarget();
    const heard = vi.fn();
    held.listen(target, "ping", heard, true);
    target.dispatchEvent(new Event("ping"));
    held.dispose();
    target.dispatchEvent(new Event("ping"));
    expect(heard).toHaveBeenCalledOnce();
  });

  it("clears its timers", () => {
    const held = new Disposables();
    const ran = vi.fn();
    held.timer().start(ran, 100);
    held.dispose();
    vi.advanceTimersByTime(1000);
    expect(ran).not.toHaveBeenCalled();
  });
});
