import { describe, expect, it } from "vitest";

// Every source but fileDialog.ts, to check that file dialogs go through it.
const sources = import.meta.glob<string>(["../**/*.{ts,tsx}", "!./fileDialog.ts", "!./fileDialog.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

describe("file dialogs", () => {
  it("are found in the sources", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(50);
  });

  it("are opened only through fileDialog.ts", () => {
    const direct = Object.entries(sources)
      .filter(([, source]) => source.includes('"@tauri-apps/plugin-dialog"'))
      .map(([path]) => path);
    expect(direct).toEqual([]);
  });
});
