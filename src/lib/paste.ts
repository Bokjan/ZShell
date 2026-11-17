/**
 * Control characters a paste may not carry: C0 but for tab, line feed and carriage return,
 * and C1. ESC could end a bracketed paste early (`ESC [201~`), so that the rest runs as typed
 * commands, and other controls act on their own once read (Ctrl+O runs the line in bash).
 * DEL stays: it only erases, as typed. As in xterm (`allowPasteControls`), iTerm2 and
 * Alacritty, a paste is only text.
 */
const PASTE_CONTROLS = /[\x00-\x08\x0b\x0c\x0e-\x1f\x80-\x9f]/g;

/** `text` as it is pasted into a terminal: without the control characters it may not carry. */
export const pastable = (text: string) => text.replace(PASTE_CONTROLS, "");
