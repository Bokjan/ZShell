import type { ReactNode } from "react";

/*
 * Every icon in the app, drawn as an SVG. Text glyphs and emoji (×, ▾, ⟳, 📁) look and sit
 * differently in each platform's fonts, so they are never used as icons; `icons.test.ts`
 * fails on an `<svg>` outside this file and on such characters in components. An icon is
 * drawn on a 16 × 16 grid with strokes in `currentColor`, so it takes the color of the text
 * around it, and is hidden from screen readers: the button or text it is in names it.
 */

interface SvgProps {
  size: number | string;
  strokeWidth?: number;
  className?: string;
  children: ReactNode;
}

function Svg({ size, strokeWidth = 1.4, className, children }: SvgProps) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/** A plus sign, for adding. */
export function PlusIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M8 3v10M3 8h10" />
    </Svg>
  );
}

/** An ×, for closing. */
export function CloseIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M4 4l8 8M12 4l-8 8" />
    </Svg>
  );
}

/** A line of text with a send arrow, for the compose bar. */
export function ComposeIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <rect x="1.5" y="4" width="13" height="8" rx="1.5" />
      <path d="M4 8h5M7.5 6.5L9 8l-1.5 1.5" />
    </Svg>
  );
}

/** A lightning bolt, for quick commands. */
export function QuickIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M9 1.5L3.5 9H8l-1 5.5L12.5 7H8l1-5.5z" />
    </Svg>
  );
}

/** A circular arrow, for reloading. */
export function RefreshIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M13.5 8A5.5 5.5 0 1 1 11.9 4.1L13.5 5.7M13.5 2.5v3.2h-3.2" />
    </Svg>
  );
}

/** A magnifying glass, for filtering. */
export function SearchIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.5 10.5L14 14" />
    </Svg>
  );
}

/** An eye, crossed out when `crossed`, for showing and hiding hidden files. */
export function EyeIcon({ size = 14, crossed = false }: { size?: number; crossed?: boolean }) {
  return (
    <Svg size={size}>
      <path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z" />
      <circle cx="8" cy="8" r="2" />
      {crossed && <path d="M2.5 13.5l11-11" />}
    </Svg>
  );
}

/** Sliders, for the settings. */
export function SettingsIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M2 4h1.4M6.6 4H14M2 8h7.4M12.6 8H14M2 12h3.4M8.6 12H14" />
      <circle cx="5" cy="4" r="1.6" />
      <circle cx="11" cy="8" r="1.6" />
      <circle cx="7" cy="12" r="1.6" />
    </Svg>
  );
}

/** Arrows up and down, for the import and export menu. */
export function ImportExportIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M5 13V3M2.5 5.5L5 3l2.5 2.5M11 3v10M8.5 10.5L11 13l2.5-2.5" />
    </Svg>
  );
}

/** An arrow out of a box, after links that open in the browser, so that it doesn't come as a surprise. */
export function ExternalLinkIcon({ size = 11 }: { size?: number }) {
  return (
    <Svg size={size} strokeWidth={1.5} className="external-link-icon">
      <path d="M12.5 9.5v3a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1h3M9.5 2.5h4v4M13.5 2.5L7.5 8.5" />
    </Svg>
  );
}

/** A question mark in a circle, for help shown on hover. */
export function HelpIcon({ size = 12 }: { size?: number }) {
  return (
    <Svg size={size} strokeWidth={1.3}>
      <circle cx="8" cy="8" r="6.5" />
      <path d="M6.2 6.3a1.9 1.9 0 0 1 3.7.4c0 1.3-1.9 1.6-1.9 2.8" />
      <circle cx="8" cy="11.6" r="0.3" fill="currentColor" />
    </Svg>
  );
}

/** An arrow pointing up or down: moving up a folder or a list, the previous or next match, uploads and downloads. */
export function ArrowIcon({ direction, size = 14 }: { direction: "up" | "down"; size?: number }) {
  return (
    <Svg size={size}>
      <path d={direction === "up" ? "M8 13V3M4 7l4-4 4 4" : "M8 3v10M4 9l4 4 4-4"} />
    </Svg>
  );
}

/**
 * A chevron: after a label, down on a button that opens a menu, up or down for a sort order
 * (`inline`, centered beside the label); alone in a button, the steps of a number field.
 */
export function ChevronIcon({ direction = "down", size = 10, inline = true }: { direction?: "up" | "down"; size?: number; inline?: boolean }) {
  return (
    <Svg size={size} strokeWidth={1.8} className={inline ? "inline-icon" : undefined}>
      <path d={direction === "up" ? "M3.5 10.5L8 6l4.5 4.5" : "M3.5 6L8 10.5 12.5 6"} />
    </Svg>
  );
}

/** A chevron pointing right, turned down when `open`: a folder in the session list. */
export function DisclosureIcon({ open, size = 10 }: { open: boolean; size?: number }) {
  return (
    <Svg size={size} strokeWidth={2.24} className={`chevron${open ? " open" : ""}`}>
      <path d="M5.6 3.2L10.4 8l-4.8 4.8" />
    </Svg>
  );
}

/** A pencil, for editing. */
export function PencilIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M10.5 2.5l3 3-8 8H2.5v-3l8-8zM9 4l3 3" />
    </Svg>
  );
}

/** A trash can, for deleting. */
export function TrashIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M2.5 4.5h11M6.5 4.5V3a.5.5 0 0 1 .5-.5h2a.5.5 0 0 1 .5.5v1.5M4 4.5l.7 8.6a1 1 0 0 0 1 .9h4.6a1 1 0 0 0 1-.9l.7-8.6M6.8 7v4.5M9.2 7v4.5" />
    </Svg>
  );
}

/** A check mark, for a menu item that is on. */
export function CheckIcon({ size = 10 }: { size?: number }) {
  return (
    <Svg size={size} strokeWidth={1.8}>
      <path d="M3 8.5l3.2 3L13 4.5" />
    </Svg>
  );
}

/** A folder, in the file list; as tall as the text (1em), which the text size setting scales. */
export function FolderIcon({ size = "1em" }: { size?: number | string }) {
  return (
    <Svg size={size} strokeWidth={1.3}>
      <path d="M1.5 4a1 1 0 0 1 1-1h3.6l1.5 1.6h5.9a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V4z" />
    </Svg>
  );
}

/** A file with a folded corner, in the file list; sized like `FolderIcon`. */
export function FileIcon({ size = "1em" }: { size?: number | string }) {
  return (
    <Svg size={size} strokeWidth={1.3}>
      <path d="M3.5 1.5h6l3 3v9a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1zM9.5 1.5v3h3" />
    </Svg>
  );
}
