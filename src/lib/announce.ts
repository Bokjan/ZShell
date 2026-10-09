/**
 * Tells screen readers about something that only shows up briefly or elsewhere in the window
 * (a tab in the background disconnecting, a transfer finishing), once they finish what they
 * are reading. One live region serves the whole app; it is created when first needed.
 */
let region: HTMLElement | null = null;

/** How long a message stays in the region; it is read when added, so only to tidy up. */
const KEEP_MS = 10_000;

export function announce(message: string) {
  if (!region) {
    region = document.createElement("div");
    region.className = "visually-hidden";
    region.setAttribute("aria-live", "polite");
    document.body.append(region);
  }
  // Each message is a new line in the region, so that two at once, or the same one twice,
  // are all read.
  const line = document.createElement("div");
  line.textContent = message;
  region.append(line);
  setTimeout(() => line.remove(), KEEP_MS);
}
