import type { TFunction } from "i18next";

import type { FoundShell, LocalLaunch, LocalOptions, LocalShell, Profile } from "../ipc/api";

/**
 * Arguments as typed in one line: separated by spaces, grouped by double quotes (removed), a
 * doubled quote inside quotes standing for one. Backslashes are taken as they are, as Windows
 * paths have them.
 */
export function parseArgs(text: string): string[] {
  const args: string[] = [];
  let current: string | null = null;
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c !== '"') current += c;
      else if (text[i + 1] === '"') {
        current += '"';
        i++;
      } else quoted = false;
    } else if (c === '"') {
      quoted = true;
      current ??= "";
    } else if (/\s/.test(c)) {
      if (current !== null) args.push(current);
      current = null;
    } else current = (current ?? "") + c;
  }
  if (current !== null) args.push(current);
  return args;
}

/** Arguments as one line that `parseArgs` reads back the same. */
export const formatArgs = (args: string[]) =>
  args.map((arg) => (arg === "" || /[\s"]/.test(arg) ? `"${arg.split('"').join('""')}"` : arg)).join(" ");

/** The file name in a path, by either separator, whichever system the path is from. */
export const programFile = (path: string) => path.split(/[\\/]/).pop() || path;

export function sameShell(a: LocalShell, b: LocalShell): boolean {
  if (a.kind === "program" && b.kind === "program")
    return a.path === b.path && a.args.length === b.args.length && a.args.every((arg, i) => arg === b.args[i]);
  if (a.kind === "wsl" && b.kind === "wsl") return a.distro === b.distro;
  return a.kind === b.kind;
}

/**
 * What a local session runs, in short, where sessions are listed with their addresses: the
 * program's file name and arguments, the WSL command, or `localhost` for the default shell.
 */
export function shellCommand(shell: LocalShell): string {
  if (shell.kind === "program") return [programFile(shell.path), formatArgs(shell.args)].filter(Boolean).join(" ");
  if (shell.kind === "wsl") return `wsl -d ${formatArgs([shell.distro])}`;
  return "localhost";
}

/** What a found shell is called: cmd in the user's language, as Windows names it. */
export const foundName = (found: FoundShell, t: TFunction): string => (found.kind === "cmd" ? t("local.cmd") : found.name);

/**
 * The title of a local terminal running `shell` without a saved session: `defaultName` for the
 * default shell, the name of the shell found here, else the distribution's or the program's.
 */
export function shellTitle(shell: LocalShell, found: FoundShell[], defaultName: string, t: TFunction): string {
  if (shell.kind === "default") return defaultName;
  const known = found.find((f) => sameShell(f.shell, shell));
  if (known) return foundName(known, t);
  if (shell.kind === "wsl") return shell.distro;
  const file = programFile(shell.path);
  return file.replace(/\.[^.]+$/, "") || file;
}

/** The local options of a saved local session; undefined for other sessions. */
export const localOptions = (profile: Profile | undefined): LocalOptions | undefined =>
  profile?.connection.protocol === "local" ? profile.connection : undefined;

/**
 * What "+" opens, by the setting (`terminal.localShell`): a saved local session (one gone, or
 * no longer local, falls back to the default shell), or a shell.
 */
export function defaultLocal(setting: LocalLaunch | null, profiles: Profile[]): { profile: Profile } | { shell: LocalShell } {
  if (setting?.kind === "shell") return { shell: setting.shell };
  const profile = setting && profiles.find((p) => p.id === setting.profileId);
  return profile && localOptions(profile) ? { profile } : { shell: { kind: "default" } };
}
