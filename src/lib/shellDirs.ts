/**
 * What the terminals tell the file panels about their shells' folders, by pane: the folder a
 * shell reported (OSC 7), and when it may have changed folders. The backend finds the folder
 * itself where the server allows (`sftp.shellDir`); a report is used where it doesn't.
 */

type Listener = () => void;

const reported = new Map<number, string>();
const listeners = new Map<number, Set<Listener>>();

const notify = (pane: number) => listeners.get(pane)?.forEach((listener) => listener());

/**
 * The path in an OSC 7 report (`file://host/path`, percent-encoded); null for anything else.
 * The host isn't checked: a shell on another machine (ssh from the terminal) may report a
 * folder that isn't on this one, which then fails to list.
 */
export function parseOsc7(data: string): string | null {
  const match = /^file:\/\/[^/]*(\/.*)$/.exec(data);
  if (!match) return null;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return null;
  }
}

export const shellDirs = {
  /** The folder the shell in the pane last reported, if it has. */
  reported: (pane: number) => reported.get(pane) ?? null,

  report(pane: number, path: string) {
    reported.set(pane, path);
    notify(pane);
  },

  /** The shell in the pane may be in another folder: a command was entered and the output has paused. */
  mayHaveMoved: notify,

  /** Calls `listener` whenever the shell in the pane may have changed folders; returns how to stop. */
  subscribe(pane: number, listener: Listener) {
    const set = listeners.get(pane) ?? new Set();
    set.add(listener);
    listeners.set(pane, set);
    return () => {
      set.delete(listener);
      if (set.size === 0) listeners.delete(pane);
    };
  },

  /** The pane's terminal is gone. */
  forget(pane: number) {
    reported.delete(pane);
  },
};
