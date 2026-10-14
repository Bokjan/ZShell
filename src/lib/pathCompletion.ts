import type { FileEntry } from "./api";

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/**
 * What a remote path being typed completes from: the folder to list, as typed up to its last
 * `/` (empty for a bare name, which, like a relative path entered, is in the SFTP session's
 * starting folder), and the start of a folder name in it.
 */
export function completionTarget(input: string): { dir: string; prefix: string } {
  const slash = input.lastIndexOf("/");
  return slash < 0 ? { dir: "", prefix: input } : { dir: input.slice(0, slash + 1), prefix: input.slice(slash + 1) };
}

/**
 * The names of the folders among `entries` (links to folders too) that start with `prefix`,
 * sorted; ignoring case when none starts with it as typed. Hidden ones only when they are shown,
 * or when `prefix` starts with a dot.
 */
export function folderCandidates(entries: FileEntry[], prefix: string, showHidden: boolean): string[] {
  const hidden = showHidden || prefix.startsWith(".");
  const folders = entries.filter((e) => e.isDir && (hidden || !e.name.startsWith("."))).map((e) => e.name);
  let names = folders.filter((name) => name.startsWith(prefix));
  if (names.length === 0) {
    const lower = prefix.toLowerCase();
    names = folders.filter((name) => name.toLowerCase().startsWith(lower));
  }
  return names.sort(collator.compare);
}

/** The longest start that all `names` share. */
export function commonPrefix(names: string[]): string {
  if (names.length === 0) return "";
  let prefix = names[0];
  for (const name of names.slice(1)) {
    let i = 0;
    while (i < prefix.length && i < name.length && prefix[i] === name[i]) i++;
    prefix = prefix.slice(0, i);
  }
  return prefix;
}

/**
 * What Tab puts in place of `prefix`, as a shell does: the only candidate as a folder (with a
 * `/`, to go on into it), or as much as all candidates share; null when that adds nothing.
 */
export function tabCompletion(prefix: string, candidates: string[]): string | null {
  if (candidates.length === 1) return `${candidates[0]}/`;
  const shared = commonPrefix(candidates);
  return shared.length > prefix.length || (shared.length === prefix.length && shared !== prefix) ? shared : null;
}
