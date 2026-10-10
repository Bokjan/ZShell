import type { TFunction } from "i18next";

import type { TerminalScheme } from "../lib/terminalSchemes";

/** The name shown for a scheme; the defaults' names are translated. */
export const schemeLabel = (scheme: TerminalScheme, t: TFunction) =>
  scheme.name ?? (scheme.dark ? t("settings.schemeDefaultDark") : t("settings.schemeDefaultLight"));

/** Miniature terminal in the scheme's colors; only a picture, so screen readers skip it. */
export function SchemePreview({ scheme }: { scheme: TerminalScheme }) {
  const t = scheme.theme;
  return (
    <span className="scheme-preview" aria-hidden="true" style={{ background: t.background, color: t.foreground }}>
      <span className="scheme-prompt">
        <span style={{ color: t.green }}>~</span> <span style={{ color: t.blue }}>$</span> ls
      </span>
      <span className="scheme-swatches">
        {[t.red, t.green, t.yellow, t.blue, t.magenta, t.cyan].map((color, i) => (
          <span key={i} style={{ background: color }} />
        ))}
      </span>
    </span>
  );
}
