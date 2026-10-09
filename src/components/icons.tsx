/** A plus sign, drawn as an SVG rather than the "+" glyph, which sits off center in some fonts (Segoe UI). */
export function PlusIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true">
      <path d="M8 3v10M3 8h10" />
    </svg>
  );
}

/** An ×, for closing; drawn for the same reason as `PlusIcon`. */
export function CloseIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true">
      <path d="M4 4l8 8M12 4l-8 8" />
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

/** A lightning bolt, for quick commands. */
export function QuickIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 1.5L3.5 9H8l-1 5.5L12.5 7H8l1-5.5z" />
    </svg>
  );
}

/** A circular arrow, for reloading; drawn because the "⟳" glyph falls back to a tiny font on macOS. */
export function RefreshIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M13.5 8A5.5 5.5 0 1 1 11.9 4.1L13.5 5.7M13.5 2.5v3.2h-3.2" />
    </svg>
  );
}

/** A magnifying glass, for filtering. */
export function SearchIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true">
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.5 10.5L14 14" />
    </svg>
  );
}

/** An eye, crossed out when `crossed`, for showing and hiding hidden files. */
export function EyeIcon({ size = 14, crossed = false }: { size?: number; crossed?: boolean }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z" />
      <circle cx="8" cy="8" r="2" />
      {crossed && <path d="M2.5 13.5l11-11" />}
    </svg>
  );
}

/** A question mark in a circle, for help shown on hover. */
export function HelpIcon({ size = 12 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" aria-hidden="true">
      <circle cx="8" cy="8" r="6.5" />
      <path d="M6.2 6.3a1.9 1.9 0 0 1 3.7.4c0 1.3-1.9 1.6-1.9 2.8" />
      <circle cx="8" cy="11.6" r="0.3" fill="currentColor" />
    </svg>
  );
}
