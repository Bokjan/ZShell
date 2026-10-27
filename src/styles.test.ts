import { describe, expect, it } from "vitest";

const sheets = import.meta.glob<string>(["./styles.css", "./styles/*.css"], { query: "?raw", import: "default", eager: true });
const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.ts", "!./ipc/bindings.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/**
 * Classes styled by a rule of their own, which takes any element with the class wherever it is
 * (`.badge`, `.splitter:hover`), not only inside another element or with another class.
 */
function globalClasses(css: string): Set<string> {
  const found = new Set<string>();
  // The arguments of `:not()`, `:is()` and the like go first: they hold commas of their own.
  const selectors = css.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\((?:[^()]|\([^()]*\))*\)/g, "");
  for (const [, list] of selectors.matchAll(/([^{}]+)\{/g)) {
    for (const selector of list.split(",")) {
      const match = /^\.([\w-]+)$/.exec(selector.trim().replace(/::?[\w-]+/g, ""));
      if (match) found.add(match[1]);
    }
  }
  return found;
}

/**
 * The words of the strings an element's classes are made of: `className="…"`, `className={…}`,
 * `.className = "…"` and `classList.add(…)`, without what a template literal interpolates
 * unless it is a string itself (`${on ? " on" : ""}`).
 */
function classWords(source: string): string[] {
  const words: string[] = [];
  for (const start of source.matchAll(/\bclassName\s*=\s*|\bclassList\.(?:add|toggle|replace)\b/g)) {
    // Brackets and quotes still open, a quote being a string and a bracket code.
    const open: string[] = [];
    let text = "";
    for (let i = start.index + start[0].length; i < source.length; i++) {
      const c = source[i];
      const inside = open[open.length - 1] as string | undefined;
      if (inside === undefined || "{([".includes(inside)) {
        if (inside === undefined && !"\"'`{(".includes(c)) break;
        if ("\"'`{([".includes(c)) open.push(c);
        else if ("})]".includes(c)) open.pop();
        else continue;
      } else if (c === "\\") i++;
      else if (c === inside || (inside === "`" && c === "$" && source[i + 1] === "{")) {
        words.push(...text.split(/\s+/).filter(Boolean));
        text = "";
        if (c === inside) {
          open.pop();
        } else {
          open.push("{");
          i++;
        }
      } else text += c;
      if (open.length === 0) break;
    }
  }
  return words;
}

/**
 * Classes styled on their own that more than one file gives elements, as the same thing each time
 * (a dialog, a field's width). Any other such class is two things that happen to share a
 * name, the rules for one drawing the other as well, as when a tab's mark for new output took the
 * position of the terminal's button back to the end: name them apart instead.
 */
const SHARED = [
  // Dialogs and their fields.
  "dialog",
  "dialog-message",
  "field-title",
  "grow",
  "import-dialog",
  "import-alias",
  "import-address",
  // Controls and messages drawn alike wherever they are.
  "segmented",
  "icon-button",
  "panel-error",
  "visually-hidden",
  // The lock screens.
  "unlock-panel",
  "unlock-actions",
  "vault-failure",
  // A session's state, wherever it is shown.
  "status-dot",
  // The sidebar's rows: sessions and folders, and the quick connect row.
  "tree-row",
  "tree-icon",
  "tree-name",
];

// Colors come from the theme's variables (see "主题" in ARCHITECTURE.md); the color field draws
// the hues it offers.
const OWN_COLORS = ["./styles/theme.css", "./styles/color-field.css"];

describe("styles", () => {
  it("are found", () => {
    expect(Object.keys(sheets).length).toBeGreaterThan(20);
  });

  it("take colors from the theme", () => {
    const found = Object.entries(sheets)
      .filter(([path]) => !OWN_COLORS.includes(path))
      .flatMap(([path, css]) =>
        css
          .replace(/\/\*[\s\S]*?\*\//g, "")
          .split("\n")
          .filter((line) => /#[0-9a-f]{3,8}\b|\b(rgb|hsl)a?\(/i.test(line))
          .map((line) => `${path}: ${line.trim()}`),
      );
    expect(found).toEqual([]);
  });

  it("name each thing they style apart from the others", () => {
    const global = new Set(Object.values(sheets).flatMap((css) => [...globalClasses(css)]));
    const users = new Map<string, Set<string>>();
    for (const [path, source] of Object.entries(sources)) {
      for (const word of classWords(source)) if (global.has(word)) users.set(word, (users.get(word) ?? new Set()).add(path));
    }
    expect(users.get("visually-hidden")?.size).toBeGreaterThan(1);
    const shared = [...users].filter(([, paths]) => paths.size > 1);
    expect(shared.filter(([name]) => !SHARED.includes(name)).map(([name, paths]) => `.${name}: ${[...paths].join(", ")}`)).toEqual([]);
    // Nor are classes listed that no longer need to be.
    expect(SHARED.filter((name) => !shared.some(([shared]) => shared === name))).toEqual([]);
  });

  it("show text fields' focus in contrast themes, which replace the border color that shows it", () => {
    expect(sheets["./styles/forced-colors.css"]).toMatch(/:is\(input[^)]*\), textarea\)[^{]*:focus/);
  });
});
