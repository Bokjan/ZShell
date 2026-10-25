import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PROMPT_SETTLE_MS, PromptWatch } from "./promptWatch";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("PromptWatch", () => {
  it("settles once the output pauses after a command is entered", () => {
    const settled = vi.fn();
    const watch = new PromptWatch(settled);
    watch.typed("cd /tmp\r");
    vi.advanceTimersByTime(PROMPT_SETTLE_MS - 100);
    watch.output();
    vi.advanceTimersByTime(PROMPT_SETTLE_MS - 1);
    expect(settled).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(settled).toHaveBeenCalledOnce();
  });

  it("waits for nothing without a line break, nor for output after it settled", () => {
    const settled = vi.fn();
    const watch = new PromptWatch(settled);
    watch.typed("ls");
    watch.output();
    vi.advanceTimersByTime(PROMPT_SETTLE_MS * 2);
    expect(settled).not.toHaveBeenCalled();
    watch.typed("\r");
    vi.advanceTimersByTime(PROMPT_SETTLE_MS);
    watch.output();
    vi.advanceTimersByTime(PROMPT_SETTLE_MS * 2);
    expect(settled).toHaveBeenCalledOnce();
  });

  it("settles nothing once disposed", () => {
    const settled = vi.fn();
    const watch = new PromptWatch(settled);
    watch.typed("\r");
    watch.dispose();
    vi.advanceTimersByTime(PROMPT_SETTLE_MS * 2);
    expect(settled).not.toHaveBeenCalled();
  });
});
