import { describe, expect, it } from "vitest";

import { KittyNotices, PointerShapes, osc52Text, parseOsc777, parseOsc9 } from "./osc";

const base64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

describe("osc52Text", () => {
  it("decodes the text of any selection as UTF-8", () => {
    expect(osc52Text(`c;${base64("hello")}`, 1024)).toBe("hello");
    expect(osc52Text(`;${base64("中文 ✓")}`, 1024)).toBe("中文 ✓");
    expect(osc52Text(`ps0;${base64("x")}`, 1024)).toBe("x");
  });

  it("never answers queries, and ignores clearing and anything malformed", () => {
    expect(osc52Text("c;?", 1024)).toBeNull();
    expect(osc52Text("c;", 1024)).toBeNull();
    expect(osc52Text("c;not base64!", 1024)).toBeNull();
    expect(osc52Text(`c;${btoa("\xff\xfe")}`, 1024)).toBeNull();
    expect(osc52Text(`x;${base64("hello")}`, 1024)).toBeNull();
    expect(osc52Text(base64("hello"), 1024)).toBeNull();
  });

  it("ignores text past the limit", () => {
    expect(osc52Text(`c;${base64("abcd")}`, 4)).toBe("abcd");
    expect(osc52Text(`c;${base64("abcde")}`, 4)).toBeNull();
    expect(osc52Text(`c;${base64("a".repeat(5000))}`, 1024)).toBeNull();
  });
});

describe("parseOsc9", () => {
  it("reads ConEmu's progress", () => {
    expect(parseOsc9("4;1;40")).toEqual({ progress: { state: "normal", percent: 40 } });
    expect(parseOsc9("4;2")).toEqual({ progress: { state: "error", percent: null } });
    expect(parseOsc9("4;3;0")).toEqual({ progress: { state: "indeterminate", percent: 0 } });
    expect(parseOsc9("4;4;250")).toEqual({ progress: { state: "paused", percent: 100 } });
    expect(parseOsc9("4;0;0")).toEqual({ progress: null });
    expect(parseOsc9("4")).toEqual({ progress: null });
  });

  it("ignores ConEmu's other commands and unknown states", () => {
    expect(parseOsc9("9;/home/alice")).toBeNull();
    expect(parseOsc9("4;7;10")).toBeNull();
    expect(parseOsc9("12")).toBeNull();
  });

  it("takes anything else as a notification", () => {
    expect(parseOsc9("Build finished")).toEqual({ notify: "Build finished" });
    expect(parseOsc9("3 tests failed")).toEqual({ notify: "3 tests failed" });
    expect(parseOsc9("  ")).toBeNull();
  });
});

describe("parseOsc777", () => {
  it("reads notifications with their title and body", () => {
    expect(parseOsc777("notify;Done;Built in 3s; no warnings")).toEqual({ title: "Done", body: "Built in 3s; no warnings" });
    expect(parseOsc777("notify;Done")).toEqual({ title: "Done", body: "" });
    expect(parseOsc777("notify;;")).toBeNull();
    expect(parseOsc777("preexec")).toBeNull();
  });
});

describe("KittyNotices", () => {
  it("shows a title by default, and a body", () => {
    const notices = new KittyNotices();
    expect(notices.feed(";Hello")).toEqual({ title: "Hello", body: "" });
    expect(notices.feed("p=body;Just text")).toEqual({ title: "", body: "Just text" });
  });

  it("gathers the parts of a notification by its id", () => {
    const notices = new KittyNotices();
    expect(notices.feed("i=1:d=0;Build")).toBeNull();
    expect(notices.feed("i=2:d=0;Other")).toBeNull();
    expect(notices.feed(`i=1:d=0:p=body:e=1;${base64("完成，")}`)).toBeNull();
    expect(notices.feed("i=1:p=body;no errors")).toEqual({ title: "Build", body: "完成，no errors" });
    expect(notices.feed("i=2:p=close;")).toBeNull();
    expect(notices.feed("i=2:p=body;Alone")).toEqual({ title: "", body: "Alone" });
  });

  it("answers nothing else, and forgets the oldest unfinished ones", () => {
    const notices = new KittyNotices();
    expect(notices.feed("i=1:p=?;")).toBeNull();
    expect(notices.feed("i=1:e=1;%%%")).toBeNull();
    for (let i = 0; i < 9; i++) notices.feed(`i=${i}:d=0;Title ${i}`);
    expect(notices.feed("i=0:p=body;x")).toEqual({ title: "", body: "x" });
    expect(notices.feed("i=8:p=body;y")).toEqual({ title: "Title 8", body: "y" });
  });
});

describe("PointerShapes", () => {
  it("sets CSS and X11 names, the first known of a list", () => {
    const shapes = new PointerShapes();
    expect(shapes.feed("pointer", "normal")).toBe(true);
    expect(shapes.shape("normal")).toBe("pointer");
    expect(shapes.feed("=hand2", "normal")).toBe(false);
    shapes.feed("fancy,sb_h_double_arrow,text", "normal");
    expect(shapes.shape("normal")).toBe("ew-resize");
    expect(shapes.feed("fancy", "normal")).toBe(false);
    expect(shapes.feed("?pointer", "normal")).toBe(false);
    shapes.feed("", "normal");
    expect(shapes.shape("normal")).toBe("");
  });

  it("keeps a stack for each screen", () => {
    const shapes = new PointerShapes();
    shapes.feed("text", "normal");
    shapes.feed(">pointer", "normal");
    shapes.feed("crosshair", "alternate");
    expect(shapes.shape("normal")).toBe("pointer");
    expect(shapes.shape("alternate")).toBe("crosshair");
    shapes.feed("<", "normal");
    expect(shapes.shape("normal")).toBe("text");
    shapes.reset();
    expect(shapes.shape("normal")).toBe("");
    expect(shapes.shape("alternate")).toBe("");
  });
});
