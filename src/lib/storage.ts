/**
 * Per-machine state kept in the page's storage: layout and conveniences (the sidebar's width,
 * the file list's view, recent sessions), never settings. Storage can fail (turned off, full):
 * a value that can't be read is the caller's default, and one that can't be stored still
 * applies until the app restarts.
 */
export function storedString(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function storeString(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Not persisted (see above).
  }
}

/** The value stored as JSON under `key`; undefined if there is none or it can't be read. */
export function storedJson(key: string): unknown {
  const text = storedString(key);
  if (text === null) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

export const storeJson = (key: string, value: unknown) => storeString(key, JSON.stringify(value));
