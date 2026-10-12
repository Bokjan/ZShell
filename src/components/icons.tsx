import type { ReactNode } from "react";

/*
 * Every icon in the app, drawn as an SVG. Text glyphs and emoji (×, ▾, ⟳, 📁) look and sit
 * differently in each platform's fonts, so they are never used as icons; `icons.test.ts`
 * fails on an `<svg>` outside this file and on such characters in components. An icon is
 * drawn on a 16 × 16 grid with strokes in `currentColor`, so it takes the color of the text
 * around it, and is hidden from screen readers: the button or text it is in names it.
 *
 * Alignment: an icon alone in a button is a block the button centers; one after text (a
 * menu's arrow, a sort arrow) has `inline-icon` in an `inline-flex` button; one in text that
 * may wrap (a help tip, an external link) sits on the middle of the capitals with
 * `vertical-align: calc(0.35em - half its height)`. Icons have fixed sizes, except the file
 * list's folder and file icons (`1em`), which follow the text size.
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

/** A gear, for the settings. Six teeth: more run together at its size. */
export function SettingsIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size}>
      <path d="M6.76 3.36L7.19 1.55H8.81L9.24 3.36L11.39 4.61L13.18 4.07L13.99 5.48L12.64 6.76V9.24L13.99 10.52L13.18 11.93L11.39 11.39L9.24 12.64L8.81 14.45H7.19L6.76 12.64L4.61 11.39L2.82 11.93L2.01 10.52L3.36 9.24V6.76L2.01 5.48L2.82 4.07L4.61 4.61Z" />
      <circle cx="8" cy="8" r="1.9" />
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
 * A solid arrow up or down, for the status bar's send and receive lights. Unlike the other
 * icons it is filled: a light is on or off, and at its small size an outline would barely
 * show the difference. The thin stroke only rounds its corners a little.
 */
export function ActivityIcon({ direction, size = 12 }: { direction: "up" | "down"; size?: number }) {
  return (
    <Svg size={size} strokeWidth={0.6}>
      <path
        d="M8 1.76L14.09 8.3H10.2V14.1H5.8V8.3H1.91Z"
        fill="currentColor"
        transform={direction === "down" ? "rotate(180 8 8)" : undefined}
      />
    </Svg>
  );
}

const CHEVRONS = { up: "M3.5 10.5L8 6l4.5 4.5", down: "M3.5 6L8 10.5 12.5 6", right: "M6 3.5L10.5 8 6 12.5" };

/**
 * A chevron: after a label, down on a button that opens a menu, up or down for a sort order
 * (`inline`, centered beside the label); alone in a button, the steps of a number field;
 * right on a menu item that opens a submenu.
 */
export function ChevronIcon({
  direction = "down",
  size = 10,
  inline = true,
}: {
  direction?: keyof typeof CHEVRONS;
  size?: number;
  inline?: boolean;
}) {
  return (
    <Svg size={size} strokeWidth={1.8} className={inline ? "inline-icon" : undefined}>
      <path d={CHEVRONS[direction]} />
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

/**
 * A session's protocol, in the session list: a terminal for SSH, and for the consoles of
 * network devices the port they are reached on, an Ethernet jack for Telnet and a serial
 * port's connector for serial.
 */
export function ProtocolIcon({ protocol, size = 14 }: { protocol: "ssh" | "telnet" | "serial"; size?: number }) {
  return (
    <Svg size={size} strokeWidth={1.3} className="protocol-icon">
      {protocol === "ssh" ? (
        <>
          <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
          <path d="M4.5 6l2.2 2-2.2 2M8.5 10.5h3" />
        </>
      ) : protocol === "telnet" ? (
        <>
          <path d="M2 3h12v7.5h-2.5v2.5h-7v-2.5H2z" />
          <path d="M5 5.5v1.6M7 5.5v1.6M9 5.5v1.6M11 5.5v1.6" />
        </>
      ) : (
        <>
          <path d="M1.5 5h13l-1.6 6.5H3.1z" />
          <path d="M5 7.3h0M8 7.3h0M11 7.3h0M6.5 9.4h0M9.5 9.4h0" strokeWidth={1.6} />
        </>
      )}
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

/** A padlock, on the unlock screen. */
export function LockIcon({ size = 14 }: { size?: number }) {
  return (
    <Svg size={size} strokeWidth={1.2}>
      <rect x="3" y="7" width="10" height="7" rx="1.5" />
      <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2M8 9.8v1.6" />
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
