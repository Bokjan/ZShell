/** The keys of punctuation on a US keyboard (`KeyboardEvent.code`), by the character they type there. */
const PUNCTUATION_CODES: Record<string, string> = {
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  ";": "Semicolon",
  "'": "Quote",
  "[": "BracketLeft",
  "]": "BracketRight",
  "\\": "Backslash",
  "=": "Equal",
  "-": "Minus",
  "`": "Backquote",
};

/** The key (`KeyboardEvent.code`) that types `char`, a lowercase letter or punctuation, on a US keyboard. */
export function usCode(char: string): string | undefined {
  return /^[a-z]$/.test(char) ? `Key${char.toUpperCase()}` : PUNCTUATION_CODES[char];
}

/** The punctuation the key `code` types on a US keyboard. */
export const usPunctuation = (code: string): string | undefined =>
  Object.keys(PUNCTUATION_CODES).find((char) => PUNCTUATION_CODES[char] === code);

/** What of a key press `isKey` reads. */
export interface KeyPress {
  key: string;
  code: string;
}

/**
 * Whether a key press is that of `char`, a lowercase letter or punctuation, for a shortcut, as
 * macOS, Windows and most apps take them: by the character the key types (`key`) where that
 * is ASCII, so the shortcut is on the key labeled so whatever the layout (AZERTY, Dvorak, the
 * brackets of a Japanese keyboard); by the key where the layout types something else there
 * (Cyrillic letters, dead keys, the characters Option types on macOS), at its place on a US
 * keyboard, as Windows and kitty do. A key typing other ASCII doesn't count: on AZERTY, the
 * comma on the US M key isn't M. Never by `keyCode`: WebKit gives 229 for it on macOS while an
 * input method is on, and none for non-Latin letters on Linux.
 */
export function isKey(e: KeyPress, char: string): boolean {
  return /^[\x21-\x7e]$/.test(e.key) ? e.key.toLowerCase() === char : e.code === usCode(char);
}
