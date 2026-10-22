import { describe, expect, it } from "vitest";

const sheets = import.meta.glob<string>(["./styles.css", "./styles/*.css"], { query: "?raw", import: "default", eager: true });

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

  it("show text fields' focus in contrast themes, which replace the border color that shows it", () => {
    expect(sheets["./styles/forced-colors.css"]).toMatch(/:is\(input[^)]*\), textarea\)[^{]*:focus/);
  });
});
