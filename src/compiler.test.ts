import { transformSync } from "@babel/core";
import reactCompiler from "babel-plugin-react-compiler";
import { describe, expect, it } from "vitest";

// Every module, as the build gives them to React Compiler (vite.config.ts).
const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.ts", "!./**/*.d.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

interface Problem {
  reason?: string;
  description?: string | null;
  loc?: { start?: { line: number } } | null;
  details?: { loc?: { start?: { line: number } } | null }[];
}

/** Where and why Compiler gave up on a function, which then runs without memoization. */
function skipped(path: string, source: string): string[] {
  const found: string[] = [];
  transformSync(source, {
    filename: path,
    babelrc: false,
    configFile: false,
    parserOpts: { plugins: ["typescript", "jsx"] },
    plugins: [
      [
        reactCompiler,
        {
          target: "19",
          panicThreshold: "none",
          logger: {
            logEvent(_file: string | null, event: { kind: string; detail?: { options?: Problem } & Problem }) {
              if (event.kind !== "CompileError" && event.kind !== "PipelineError") return;
              const problem: Problem = event.detail?.options ?? event.detail ?? {};
              const line = problem.details?.find((detail) => detail.loc?.start)?.loc?.start?.line ?? problem.loc?.start?.line;
              found.push(`${path}:${line ?? "?"}: ${problem.reason ?? event.kind}`);
            },
          },
        },
      ],
    ],
  });
  return found;
}

describe("React Compiler", () => {
  it("finds the sources", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(100);
  });

  // A component it skips loses its memoization without a word. The usual causes and how they
  // are written instead are in "渲染与 memo" in docs/ARCHITECTURE.md; one that really can't
  // compile says "use no memo" and why.
  it("compiles every component and hook", { timeout: 120_000 }, () => {
    expect(Object.entries(sources).flatMap(([path, source]) => skipped(path, source))).toEqual([]);
  });
});
