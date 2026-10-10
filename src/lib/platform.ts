export const isMac = navigator.userAgent.includes("Mac");

/**
 * Whether a key belongs to an input method's composition (Chinese, Japanese…): its Enter
 * confirms a candidate and its Escape cancels one, rather than submitting or closing. WebKit
 * ends the composition before the confirming Enter's keydown, which then only `keyCode` 229
 * tells apart.
 */
export function isComposing(e: KeyboardEvent | { nativeEvent: KeyboardEvent }): boolean {
  const event = "nativeEvent" in e ? e.nativeEvent : e;
  return event.isComposing || event.keyCode === 229;
}
/** Windows draws its own window buttons (see `WindowControls`). */
export const isWindows = navigator.userAgent.includes("Windows");
