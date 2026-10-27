import { describe, expect, it } from "vitest";

import {
  HISTORY_LIMIT,
  historyKey,
  historyKeyLabel,
  NO_HISTORY,
  stepFrom,
  visited,
  wentTo,
  type FolderHistory,
  type HistoryKey,
} from "./folderHistory";

const history = (paths: string[], index = paths.length - 1): FolderHistory => ({ paths, index });

describe("visited", () => {
  it("adds the folder after the current one", () => {
    expect(visited(NO_HISTORY, "/home")).toEqual(history(["/home"]));
    expect(visited(history(["/home"]), "/etc")).toEqual(history(["/home", "/etc"]));
  });

  it("drops the folders ahead", () => {
    expect(visited(history(["/a", "/b", "/c"], 0), "/d")).toEqual(history(["/a", "/d"]));
  });

  it("changes nothing for the current folder shown again", () => {
    const h = history(["/a", "/b"], 0);
    expect(visited(h, "/a")).toBe(h);
  });

  it("keeps only the latest folders", () => {
    let h = NO_HISTORY;
    for (let i = 0; i <= HISTORY_LIMIT; i++) h = visited(h, `/${i}`);
    expect(h.paths).toHaveLength(HISTORY_LIMIT);
    expect(h.paths[0]).toBe("/1");
    expect(h.index).toBe(HISTORY_LIMIT - 1);
  });
});

describe("stepFrom", () => {
  const h = history(["/a", "/b", "/c"], 1);

  it("finds the folder before and after", () => {
    expect(stepFrom(h, 1, -1)).toEqual({ index: 0, path: "/a" });
    expect(stepFrom(h, 1, 1)).toEqual({ index: 2, path: "/c" });
  });

  it("finds none past either end", () => {
    expect(stepFrom(h, 0, -1)).toBeNull();
    expect(stepFrom(h, 2, 1)).toBeNull();
    expect(stepFrom(NO_HISTORY, -1, 1)).toBeNull();
  });
});

describe("wentTo", () => {
  it("moves to the folder, keeping the others", () => {
    expect(wentTo(history(["/a", "/b", "/c"]), 0, "/a")).toEqual(history(["/a", "/b", "/c"], 0));
  });

  it("keeps the path the folder was listed at", () => {
    expect(wentTo(history(["/a", "/link", "/c"]), 1, "/target")).toEqual(history(["/a", "/target", "/c"], 1));
  });

  it("ignores a place no longer in the history", () => {
    const h = history(["/a"]);
    expect(wentTo(h, 3, "/x")).toBe(h);
  });
});

describe("historyKey", () => {
  const press = (key: string, code: string, mods: Partial<HistoryKey> = {}): HistoryKey => ({
    key,
    code,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...mods,
  });

  it("takes Cmd and the brackets on macOS, by the character", () => {
    expect(historyKey(press("[", "BracketLeft", { metaKey: true }), true)).toBe(-1);
    expect(historyKey(press("]", "BracketRight", { metaKey: true }), true)).toBe(1);
    // A Japanese keyboard: [ on the US ] key, ] on the US \ key.
    expect(historyKey(press("[", "BracketRight", { metaKey: true }), true)).toBe(-1);
    expect(historyKey(press("]", "Backslash", { metaKey: true }), true)).toBe(1);
    expect(historyKey(press("@", "BracketLeft", { metaKey: true }), true)).toBeNull();
  });

  it("goes by the key where the layout has no Latin letters there", () => {
    expect(historyKey(press("х", "BracketLeft", { metaKey: true }), true)).toBe(-1);
  });

  it("takes Alt and the arrows elsewhere", () => {
    expect(historyKey(press("ArrowLeft", "ArrowLeft", { altKey: true }), false)).toBe(-1);
    expect(historyKey(press("ArrowRight", "ArrowRight", { altKey: true }), false)).toBe(1);
    expect(historyKey(press("[", "BracketLeft", { metaKey: true }), false)).toBeNull();
  });

  it("ignores other modifiers", () => {
    expect(historyKey(press("[", "BracketLeft"), true)).toBeNull();
    expect(historyKey(press("[", "BracketLeft", { metaKey: true, shiftKey: true }), true)).toBeNull();
    expect(historyKey(press("[", "BracketLeft", { metaKey: true, ctrlKey: true }), true)).toBeNull();
    expect(historyKey(press("ArrowLeft", "ArrowLeft", { altKey: true, ctrlKey: true }), false)).toBeNull();
    expect(historyKey(press("ArrowLeft", "ArrowLeft"), false)).toBeNull();
  });
});

describe("historyKeyLabel", () => {
  it("writes the keys as each platform does", () => {
    expect(historyKeyLabel(-1, true, "Alt")).toBe("⌘[");
    expect(historyKeyLabel(1, true, "Alt")).toBe("⌘]");
    expect(historyKeyLabel(-1, false, "Alt")).toBe("Alt+←");
    expect(historyKeyLabel(1, false, "Alt")).toBe("Alt+→");
  });
});
