import { isMac } from "./platform";

/**
 * The app's keyboard shortcuts: the keys of each action on macOS and on other platforms
 * (Windows, and Linux, which follows it). Keys are written as on a US keyboard
 * (`Cmd+Shift+E`, `Alt+Shift+=`) and matched by `KeyboardEvent.code`, the physical key, so
 * they stay in place on other layouts. Outside macOS, keys the shell reads (Ctrl+F, Ctrl+T,
 * Ctrl+W…) get Shift added. The first keys of an action are the ones menus and tooltips show;
 * the list of shortcuts in the settings shows all of them.
 *
 * Every shortcut is matched, labeled and listed from this table, so changing one here
 * changes it everywhere, except for the native macOS menu items (`MAC_MENU`), whose keys are
 * also set in lib.rs.
 */
const KEYMAP = {
  settings: { mac: "Cmd+,", other: "Ctrl+," },
  searchSessions: { mac: "Cmd+K", other: "Ctrl+Shift+K" },
  newLocalTerminal: { mac: "Cmd+T", other: "Ctrl+Shift+T" },
  // As in browsers, on both platforms.
  nextTab: { mac: "Ctrl+Tab", other: "Ctrl+Tab" },
  previousTab: { mac: "Ctrl+Shift+Tab", other: "Ctrl+Shift+Tab" },
  tab1: { mac: "Cmd+1", other: "Alt+1" },
  tab2: { mac: "Cmd+2", other: "Alt+2" },
  tab3: { mac: "Cmd+3", other: "Alt+3" },
  tab4: { mac: "Cmd+4", other: "Alt+4" },
  tab5: { mac: "Cmd+5", other: "Alt+5" },
  tab6: { mac: "Cmd+6", other: "Alt+6" },
  tab7: { mac: "Cmd+7", other: "Alt+7" },
  tab8: { mac: "Cmd+8", other: "Alt+8" },
  lastTab: { mac: "Cmd+9", other: "Alt+9" },
  // The focused pane of a split tab.
  closeTab: { mac: "Cmd+W", other: "Ctrl+Shift+W" },
  closeWindow: { mac: "Cmd+Shift+W" },
  // As in iTerm2 on macOS and Windows Terminal elsewhere.
  splitRight: { mac: "Cmd+D", other: "Alt+Shift+=" },
  splitDown: { mac: "Cmd+Shift+D", other: "Alt+Shift+-" },
  // Elsewhere with Ctrl, since shells read Alt + arrow (moving by words).
  focusLeft: { mac: "Alt+Cmd+Left", other: "Ctrl+Alt+Left" },
  focusUp: { mac: "Alt+Cmd+Up", other: "Ctrl+Alt+Up" },
  focusRight: { mac: "Alt+Cmd+Right", other: "Ctrl+Alt+Right" },
  focusDown: { mac: "Alt+Cmd+Down", other: "Ctrl+Alt+Down" },
  // In the terminal; text fields keep their own clipboard keys.
  copy: { mac: "Cmd+C", other: ["Ctrl+Shift+C", "Ctrl+Insert"] },
  // Plain Ctrl+C copies only while text is selected, as in Windows Terminal.
  copySelection: { other: "Ctrl+C" },
  paste: { mac: "Cmd+V", other: ["Ctrl+Shift+V", "Ctrl+V", "Shift+Insert"] },
  // Ctrl+A belongs to the shell.
  selectAll: { mac: "Cmd+A" },
  find: { mac: "Cmd+F", other: "Ctrl+Shift+F" },
  composeBar: { mac: "Cmd+Shift+I", other: "Ctrl+Shift+I" },
  quickCommands: { mac: "Cmd+Shift+J", other: "Ctrl+Shift+J" },
  filePanel: { mac: "Cmd+Shift+E", other: "Ctrl+Shift+E" },
  forwardsPanel: { mac: "Cmd+Shift+P", other: "Ctrl+Shift+P" },
} satisfies Record<string, { mac?: string | string[]; other?: string | string[] }>;

export type Action = keyof typeof KEYMAP;

/** On macOS these are native menu items (lib.rs), whose keys never reach the web view. */
const MAC_MENU: readonly Action[] = ["closeTab", "closeWindow", "copy", "paste", "selectAll"];

interface Chord {
  code: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

/** Keys written by name or as the character they type, by their `code`. */
const NAMED_KEYS: Record<string, string> = {
  ",": "Comma",
  "=": "Equal",
  "-": "Minus",
  Tab: "Tab",
  Enter: "Enter",
  Insert: "Insert",
  Left: "ArrowLeft",
  Up: "ArrowUp",
  Right: "ArrowRight",
  Down: "ArrowDown",
};

/** Keys of the numeric keypad that count as the main keyboard's. */
const KEYPAD: Record<string, string> = { NumpadAdd: "Equal", NumpadSubtract: "Minus", NumpadEnter: "Enter" };

function parse(keys: string): Chord {
  const parts = keys.split("+");
  const key = parts.pop()!;
  const code = /^[A-Z]$/.test(key) ? `Key${key}` : /^[0-9]$/.test(key) ? `Digit${key}` : /^F[0-9]+$/.test(key) ? key : NAMED_KEYS[key];
  if (!code || parts.some((part) => !["Ctrl", "Alt", "Shift", "Cmd"].includes(part))) throw new Error(`Invalid shortcut: ${keys}`);
  const has = (modifier: string) => parts.includes(modifier);
  return { code, ctrl: has("Ctrl"), alt: has("Alt"), shift: has("Shift"), meta: has("Cmd") };
}

function chordsOf(action: Action, mac: boolean): Chord[] {
  const keys: string | string[] | undefined = (KEYMAP[action] as { mac?: string | string[]; other?: string | string[] })[mac ? "mac" : "other"];
  return (keys === undefined ? [] : Array.isArray(keys) ? keys : [keys]).map(parse);
}

const CHORDS = new Map((Object.keys(KEYMAP) as Action[]).map((action) => [action, chordsOf(action, isMac)]));

const sameChord = (e: KeyboardEvent, chord: Chord) =>
  (KEYPAD[e.code] ?? e.code) === chord.code &&
  e.ctrlKey === chord.ctrl &&
  e.altKey === chord.alt &&
  e.shiftKey === chord.shift &&
  e.metaKey === chord.meta;

/** Whether `e` is a shortcut of `action` that the web view handles. */
export function matches(e: KeyboardEvent, action: Action): boolean {
  if (isMac && MAC_MENU.includes(action)) return false;
  return CHORDS.get(action)!.some((chord) => sameChord(e, chord));
}

/** The first of `actions` that `e` is a shortcut of. */
export function actionOf<A extends Action>(e: KeyboardEvent, actions: readonly A[]): A | null {
  return actions.find((action) => matches(e, action)) ?? null;
}

const TAB_ACTIONS = ["tab1", "tab2", "tab3", "tab4", "tab5", "tab6", "tab7", "tab8"] as const;

/** The index of the tab `e` goes to (⌘1–⌘8 / Alt+1–Alt+8), or -1 for the last tab (⌘9 / Alt+9). */
export function tabIndexOf(e: KeyboardEvent): number | null {
  if (matches(e, "lastTab")) return -1;
  const action = actionOf(e, TAB_ACTIONS);
  return action ? TAB_ACTIONS.indexOf(action) : null;
}

/**
 * A clipboard key in the terminal, outside macOS (where the native Edit menu has them):
 * copying, pasting, or plain Ctrl+C, which copies only while there is a selection (the caller
 * checks).
 */
export function clipboardKeyOf(e: KeyboardEvent): "copy" | "copyIfSelected" | "paste" | null {
  const action = actionOf(e, ["copy", "paste", "copySelection"]);
  return action === "copySelection" ? "copyIfSelected" : action;
}

const SIDES = { focusLeft: "left", focusUp: "up", focusRight: "right", focusDown: "down" } as const;

/** The side of the pane that `e` moves the focus to. */
export function paneSideOf(e: KeyboardEvent): (typeof SIDES)[keyof typeof SIDES] | null {
  const action = actionOf(e, Object.keys(SIDES) as (keyof typeof SIDES)[]);
  return action && SIDES[action];
}

const MAC_KEY_LABELS: Record<string, string> = {
  Tab: "⇥",
  Enter: "↩",
  ArrowLeft: "←",
  ArrowUp: "↑",
  ArrowRight: "→",
  ArrowDown: "↓",
};

const OTHER_KEY_LABELS: Record<string, string> = {
  ArrowLeft: "←",
  ArrowUp: "↑",
  ArrowRight: "→",
  ArrowDown: "↓",
};

function keyLabel(code: string, mac: boolean): string {
  const named = (mac ? MAC_KEY_LABELS : OTHER_KEY_LABELS)[code];
  if (named) return named;
  const letterOrDigit = /^(?:Key|Digit)(.)$/.exec(code);
  if (letterOrDigit) return letterOrDigit[1];
  return Object.keys(NAMED_KEYS).find((key) => NAMED_KEYS[key] === code) ?? code;
}

/** A chord's modifiers and key, as macOS (⌃⌥⇧⌘) or Windows ("Ctrl+Alt+Shift") write them. */
function parts(chord: Chord, mac: boolean): { modifiers: string; key: string } {
  const key = keyLabel(chord.code, mac);
  if (mac) {
    const modifiers = `${chord.ctrl ? "⌃" : ""}${chord.alt ? "⌥" : ""}${chord.shift ? "⇧" : ""}${chord.meta ? "⌘" : ""}`;
    return { modifiers, key };
  }
  const modifiers = [chord.ctrl && "Ctrl", chord.alt && "Alt", chord.shift && "Shift"].filter(Boolean).join("+");
  return { modifiers, key };
}

function format(chord: Chord, mac: boolean): string {
  const { modifiers, key } = parts(chord, mac);
  return mac || !modifiers ? `${modifiers}${key}` : `${modifiers}+${key}`;
}

/** Every key of `action` on a platform (macOS when `mac`), as written there. */
export function labelsOn(action: Action, mac: boolean): string[] {
  return chordsOf(action, mac).map((chord) => format(chord, mac));
}

/** The keys of `action` on this platform for menus and tooltips: "⇧⌘E", "Ctrl+Shift+E". */
export function shortcutLabel(action: Action): string | undefined {
  return labelsOn(action, isMac)[0];
}

/** A row of the list of shortcuts in the settings, described by `settings.shortcutActions.<id>`. */
export interface ShortcutRow {
  id:
    | "searchSessions"
    | "newLocalTerminal"
    | "nextTab"
    | "previousTab"
    | "goToTab"
    | "lastTab"
    | "closeTab"
    | "closeWindow"
    | "splitRight"
    | "splitDown"
    | "focusPane"
    | "copy"
    | "copySelection"
    | "paste"
    | "find"
    | "composeBar"
    | "quickCommands"
    | "filePanel"
    | "forwardsPanel"
    | "settings";
  /** Only where local terminals can be opened. */
  local?: boolean;
  keys: string;
}

/**
 * The list of shortcuts in the settings, for a platform (macOS when `mac`); actions without
 * keys there (closing the window outside macOS, plain Ctrl+C on macOS) are left out.
 */
export function shortcutRows(mac = isMac): ShortcutRow[] {
  const all = (action: Action) => labelsOn(action, mac).join(", ");
  const tabs = labelsOn("tab1", mac)[0] && `${labelsOn("tab1", mac)[0]} – ${labelsOn("tab8", mac)[0]}`;
  // One set of modifiers for the four arrows: "⌥⌘ ← ↑ → ↓", "Ctrl+Alt+← ↑ → ↓".
  const arrows = (["focusLeft", "focusUp", "focusRight", "focusDown"] as const).map((action) => parts(chordsOf(action, mac)[0], mac));
  const focus = `${arrows[0].modifiers}${mac ? " " : "+"}${arrows.map((arrow) => arrow.key).join(" ")}`;
  const rows: ShortcutRow[] = [
    { id: "searchSessions", keys: all("searchSessions") },
    { id: "newLocalTerminal", keys: all("newLocalTerminal"), local: true },
    { id: "nextTab", keys: all("nextTab") },
    { id: "previousTab", keys: all("previousTab") },
    { id: "goToTab", keys: tabs },
    { id: "lastTab", keys: all("lastTab") },
    { id: "closeTab", keys: all("closeTab") },
    { id: "closeWindow", keys: all("closeWindow") },
    { id: "splitRight", keys: all("splitRight") },
    { id: "splitDown", keys: all("splitDown") },
    { id: "focusPane", keys: focus },
    { id: "copy", keys: all("copy") },
    { id: "copySelection", keys: all("copySelection") },
    { id: "paste", keys: all("paste") },
    { id: "find", keys: all("find") },
    { id: "composeBar", keys: all("composeBar") },
    { id: "quickCommands", keys: all("quickCommands") },
    { id: "filePanel", keys: all("filePanel") },
    { id: "forwardsPanel", keys: all("forwardsPanel") },
    { id: "settings", keys: all("settings") },
  ];
  return rows.filter((row) => row.keys);
}
