import type { MenuSection } from "../ipc/bindings";
import { isKey, usCode, usPunctuation } from "./keys";
import { isMac } from "./platform";

export type { MenuSection } from "../ipc/bindings";

/**
 * The app's actions and their keyboard shortcuts: the keys of each action on macOS and on
 * other platforms (Windows, and Linux, which follows it). Keys are written as on a US
 * keyboard (`Cmd+Shift+E`, `Ctrl+,`). Letters and punctuation are matched by the character
 * they type (`isKey`), so a shortcut is on the key labeled so whatever the layout, as in the
 * system's own menus. Digits are matched by the key (`KeyboardEvent.code`), since they are on
 * the same row on every layout but typed with Shift on AZERTY, and so are named keys (Tab, the
 * arrows) and punctuation with Shift, whose character with Shift depends on the layout (the
 * `=` key of a Japanese keyboard is Shift and `-`). Outside macOS, keys the shell reads (Ctrl+F,
 * Ctrl+T, Ctrl+W…) get Shift added. The first keys of an action are the ones menus and
 * tooltips show; the list of shortcuts in the settings shows all of them. Some actions have
 * no keys, and are only chosen from the app menu (`appMenu`).
 *
 * Every shortcut is matched, labeled and listed from this table, and the app menu (the native
 * one on macOS too) is built from it, so changing one here changes it everywhere.
 */
const KEYMAP = {
  settings: { mac: "Cmd+,", other: "Ctrl+," },
  // As in 1Password.
  lock: { mac: "Cmd+Shift+L", other: "Ctrl+Shift+L" },
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
  // As in iTerm2 on macOS, and elsewhere in Tabby, whose keys for the focus are these too;
  // then Windows Terminal's, the only ones before 3.16.
  splitRight: { mac: "Cmd+D", other: ["Ctrl+Shift+S", "Alt+Shift+="] },
  splitDown: { mac: "Cmd+Shift+D", other: ["Ctrl+Shift+D", "Alt+Shift+-"] },
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
  // The session list: as Finder's sidebar on macOS; elsewhere VS Code's Ctrl+B with Shift added.
  sidebar: { mac: "Ctrl+Cmd+S", other: "Ctrl+Shift+B" },
  // As in iTerm2 on macOS and Windows Terminal elsewhere.
  fullScreen: { mac: "Cmd+Enter", other: "F11" },
  // Outside macOS, which has the menu bar instead: the menu of the app's icon in the title bar.
  appMenu: { other: "Ctrl+Shift+M" },
  // Only in the app menu.
  newSession: {},
  importSshConfig: {},
  importSessions: {},
  exportSessions: {},
  checkForUpdates: {},
  releaseNotes: {},
  reportIssue: {},
  debugLog: {},
  about: {},
  // Outside macOS, whose app menu has the system's own Quit (⌘Q).
  quit: {},
} satisfies Record<string, { mac?: string | string[]; other?: string | string[] }>;

export type Action = keyof typeof KEYMAP;

/** The actions in the app menu, each labeled by `appMenu.items.<action>`. */
export type MenuAction = Extract<
  Action,
  | "settings"
  | "lock"
  | "newLocalTerminal"
  | "newSession"
  | "searchSessions"
  | "importSshConfig"
  | "importSessions"
  | "exportSessions"
  | "closeTab"
  | "closeWindow"
  | "copy"
  | "paste"
  | "find"
  | "splitRight"
  | "splitDown"
  | "filePanel"
  | "forwardsPanel"
  | "sidebar"
  | "composeBar"
  | "quickCommands"
  | "fullScreen"
  | "checkForUpdates"
  | "releaseNotes"
  | "reportIssue"
  | "debugLog"
  | "about"
  | "quit"
>;

export type MenuEntry = MenuAction | "separator";

/**
 * The app menu, on macOS (`mac`) and elsewhere. On macOS each section is a menu of the menu
 * bar, to which menu.rs adds the system's own items (Services, Hide and Quit in the app menu;
 * Undo to Select All in Edit; the Window menu). Elsewhere the menu of the app's icon in
 * the title bar has the sections as submenus, and the "app" section's items after them.
 */
export function appMenu(mac = isMac): { section: MenuSection; items: MenuEntry[] }[] {
  const file: MenuEntry[] = [
    "newLocalTerminal",
    "newSession",
    "searchSessions",
    "separator",
    "importSshConfig",
    "importSessions",
    "exportSessions",
    "separator",
    "closeTab",
    "closeWindow",
  ];
  const view: MenuEntry[] = [
    "splitRight",
    "splitDown",
    "separator",
    "sidebar",
    "filePanel",
    "forwardsPanel",
    "composeBar",
    "quickCommands",
    "separator",
    "fullScreen",
  ];
  if (mac) {
    return [
      { section: "app", items: ["about", "separator", "checkForUpdates", "separator", "settings", "separator", "lock"] },
      { section: "file", items: file },
      { section: "edit", items: ["find"] },
      { section: "view", items: view },
      { section: "help", items: ["releaseNotes", "reportIssue", "separator", "debugLog"] },
    ];
  }
  return [
    { section: "file", items: file },
    { section: "edit", items: ["copy", "paste", "separator", "find"] },
    { section: "view", items: view },
    { section: "help", items: ["releaseNotes", "reportIssue", "separator", "debugLog", "separator", "checkForUpdates", "about"] },
    { section: "app", items: ["lock", "settings", "separator", "quit"] },
  ];
}

/**
 * On macOS the items of the menu bar, whose keys go to the menu rather than to the page (the
 * page runs the action when the menu says so, see `runAction`): the system's Copy, Paste and
 * Select All, there from the start, and the app menu's once the page has put them there
 * (`setNativeMenu`). Until then, or if that failed, the page takes their keys itself.
 */
const SYSTEM_MENU = new Set<Action>(["copy", "paste", "selectAll"]);
const APP_MENU = new Set<Action>(appMenu(true).flatMap(({ items }) => items.filter((item) => item !== "separator")));
let nativeMenu = false;

/** Says that the macOS menu bar has the app menu's items now (see `useNativeMenu`). */
export function setNativeMenu() {
  nativeMenu = true;
}

interface Chord {
  /** The letter or punctuation the key types, matched as `isKey` says; null for one matched by `code`. */
  char: string | null;
  code: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

/** Keys written by name, by their `code`. */
const NAMED_KEYS: Record<string, string> = {
  Tab: "Tab",
  Enter: "Enter",
  Insert: "Insert",
  Left: "ArrowLeft",
  Up: "ArrowUp",
  Right: "ArrowRight",
  Down: "ArrowDown",
};

/** Keys of the numeric keypad that count as the main keyboard's, where they are matched by `code`. */
const KEYPAD: Record<string, string> = { NumpadAdd: "Equal", NumpadSubtract: "Minus", NumpadEnter: "Enter" };

function parse(keys: string): Chord {
  const parts = keys.split("+");
  const key = parts.pop()!;
  const has = (modifier: string) => parts.includes(modifier);
  const letter = /^[A-Z]$/.test(key);
  const punctuation = !/^[a-z]$/.test(key) ? usCode(key) : undefined;
  const code = letter
    ? `Key${key}`
    : (punctuation ?? (/^[0-9]$/.test(key) ? `Digit${key}` : /^F[0-9]+$/.test(key) ? key : NAMED_KEYS[key]));
  if (!code || parts.some((part) => !["Ctrl", "Alt", "Shift", "Cmd"].includes(part))) throw new Error(`Invalid shortcut: ${keys}`);
  const shift = has("Shift");
  // Letters, and punctuation without Shift, by the character they type (see `KEYMAP`).
  const char = letter ? key.toLowerCase() : punctuation && !shift ? key : null;
  return { char, code, ctrl: has("Ctrl"), alt: has("Alt"), shift, meta: has("Cmd") };
}

function chordsOf(action: Action, mac: boolean): Chord[] {
  const keys: string | string[] | undefined = (KEYMAP[action] as { mac?: string | string[]; other?: string | string[] })[
    mac ? "mac" : "other"
  ];
  return (keys === undefined ? [] : Array.isArray(keys) ? keys : [keys]).map(parse);
}

const CHORDS = new Map((Object.keys(KEYMAP) as Action[]).map((action) => [action, chordsOf(action, isMac)]));

const sameChord = (e: KeyboardEvent, chord: Chord) =>
  (chord.char ? isKey(e, chord.char) : (KEYPAD[e.code] ?? e.code) === chord.code) &&
  e.ctrlKey === chord.ctrl &&
  e.altKey === chord.alt &&
  e.shiftKey === chord.shift &&
  e.metaKey === chord.meta;

/** Key events that stand for an action chosen from a menu (see `actionEvent`). */
const chosen = new WeakMap<KeyboardEvent, Action>();

/**
 * A key event standing for `action`, chosen from a menu: shortcut handlers take it for one
 * of the action's keys, so a menu runs an action as its shortcut would, with the same rules
 * (dialogs, the app lock, which terminal is active).
 */
export function actionEvent(action: Action): KeyboardEvent {
  const e = new KeyboardEvent("keydown");
  chosen.set(e, action);
  return e;
}

/** The action `e` stands for, if it was chosen from a menu rather than typed. */
export const chosenAction = (e: KeyboardEvent): Action | null => chosen.get(e) ?? null;

/** Whether `e` is a shortcut of `action` that the web view handles, or the action chosen from a menu. */
export function matches(e: KeyboardEvent, action: Action): boolean {
  const choice = chosen.get(e);
  if (choice) return choice === action;
  if (isMac && (SYSTEM_MENU.has(action) || (nativeMenu && APP_MENU.has(action)))) return false;
  return CHORDS.get(action)!.some((chord) => sameChord(e, chord));
}

/**
 * Whether `e` is the key of an app menu item in the macOS menu bar (`mac`, once the page has
 * put them there: `appMenuThere`), which the page must leave to the menu (see `useShortcuts`).
 * The system's Copy, Paste and Select All are not among them: the terminal and text fields
 * handle their keys themselves (⌘A selects all of the terminal).
 */
export function toAppMenu(e: KeyboardEvent, mac = isMac, appMenuThere = nativeMenu): boolean {
  if (!mac || !appMenuThere || chosen.has(e)) return false;
  return [...APP_MENU].some((action) => (mac === isMac ? CHORDS.get(action)! : chordsOf(action, mac)).some((chord) => sameChord(e, chord)));
}

/** The first of `actions` that `e` is a shortcut of. */
export function actionOf<A extends Action>(e: KeyboardEvent, actions: readonly A[]): A | null {
  return actions.find((action) => matches(e, action)) ?? null;
}

/** The actions going to the first tabs (⌘1–⌘8 / Alt+1–Alt+8), in order; `lastTab` goes to the last. */
export const TAB_ACTIONS = ["tab1", "tab2", "tab3", "tab4", "tab5", "tab6", "tab7", "tab8"] as const;

/**
 * A clipboard key in the terminal, outside macOS (where the native Edit menu has them):
 * copying, pasting, or plain Ctrl+C, which copies only while there is a selection (the caller
 * checks).
 */
export function clipboardKeyOf(e: KeyboardEvent): "copy" | "copyIfSelected" | "paste" | null {
  const action = actionOf(e, ["copy", "paste", "copySelection"]);
  return action === "copySelection" ? "copyIfSelected" : action;
}

/** The actions moving the focus to the pane on a side, and that side. */
export const PANE_SIDES = { focusLeft: "left", focusUp: "up", focusRight: "right", focusDown: "down" } as const;

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

/** Keys whose names, outside macOS, are written in the interface's language ("Strg" in German). */
export const KEY_NAMES = ["ctrl", "alt", "shift", "tab", "enter", "insert"] as const;
export type KeyName = (typeof KEY_NAMES)[number];

let keyNames: Record<KeyName, string> = { ctrl: "Ctrl", alt: "Alt", shift: "Shift", tab: "Tab", enter: "Enter", insert: "Insert" };

/** The names of keys in the interface's language, as its keyboards and Windows write them. */
export function setKeyNames(names: Record<KeyName, string>) {
  keyNames = names;
}

const NAMED_CODES: Record<string, KeyName> = { Tab: "tab", Enter: "enter", Insert: "insert" };

function keyLabel(code: string, mac: boolean): string {
  const named = mac ? MAC_KEY_LABELS[code] : (OTHER_KEY_LABELS[code] ?? (NAMED_CODES[code] && keyNames[NAMED_CODES[code]]));
  if (named) return named;
  const letterOrDigit = /^(?:Key|Digit)(.)$/.exec(code);
  if (letterOrDigit) return letterOrDigit[1];
  return usPunctuation(code) ?? Object.keys(NAMED_KEYS).find((key) => NAMED_KEYS[key] === code) ?? code;
}

/** A chord's modifiers and key, as macOS (⌃⌥⇧⌘) or Windows ("Ctrl+Alt+Shift") write them. */
function parts(chord: Chord, mac: boolean): { modifiers: string; key: string } {
  const key = keyLabel(chord.code, mac);
  if (mac) {
    const modifiers = `${chord.ctrl ? "⌃" : ""}${chord.alt ? "⌥" : ""}${chord.shift ? "⇧" : ""}${chord.meta ? "⌘" : ""}`;
    return { modifiers, key };
  }
  const modifiers = [chord.ctrl && keyNames.ctrl, chord.alt && keyNames.alt, chord.shift && keyNames.shift].filter(Boolean).join("+");
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

/** The first macOS keys of `action` as written in the table ("Cmd+Shift+E"), for a native menu item. */
export function macKeys(action: Action): string | null {
  const keys: string | string[] | undefined = (KEYMAP[action] as { mac?: string | string[] }).mac;
  return (Array.isArray(keys) ? keys[0] : keys) ?? null;
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
    | "sidebar"
    | "fullScreen"
    | "lock"
    | "settings"
    | "appMenu";
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
    { id: "sidebar", keys: all("sidebar") },
    { id: "fullScreen", keys: all("fullScreen") },
    { id: "lock", keys: all("lock") },
    { id: "settings", keys: all("settings") },
    { id: "appMenu", keys: all("appMenu") },
  ];
  return rows.filter((row) => row.keys);
}
