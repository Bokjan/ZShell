/** A plus sign, drawn as an SVG rather than the "+" glyph, which sits off center in some fonts (Segoe UI). */
export function PlusIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true">
      <path d="M8 3v10M3 8h10" />
    </svg>
  );
}

/** A line of text with a send arrow, for the compose bar. */
export function ComposeIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="1.5" y="4" width="13" height="8" rx="1.5" />
      <path d="M4 8h5M7.5 6.5L9 8l-1.5 1.5" />
    </svg>
  );
}
