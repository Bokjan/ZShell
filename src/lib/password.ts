/**
 * What saving does with a stored password (of a session or a proxy): undefined keeps it, ""
 * deletes it, anything else replaces it. `keeps` is whether the item uses one with its
 * settings; one that no longer does has its stored one deleted if it `existed` before.
 */
export function passwordUpdate(options: { keeps: boolean; existed: boolean; clear: boolean; password: string }): string | undefined {
  const { keeps, existed, clear, password } = options;
  if (!keeps) return existed ? "" : undefined;
  if (clear) return "";
  return password || undefined;
}
