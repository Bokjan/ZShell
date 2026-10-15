import { listProfiles, proxies as proxyApi, tree, type Folder, type Profile, type Proxy, type TreeItem } from "../ipc/api";
import type { Pane } from "./panes";
import { RECENT_LIMIT, addRecent, storedRecent } from "./sessions";

/**
 * The session directory: the saved sessions, their folders and the proxies, as this window
 * last read them, and the sessions opened recently. One per window, like the tabs, but read
 * by any component that shows them rather than passed down from `App`.
 */
export interface DirectoryState {
  /** False until the first read has come back (the proxies are then unknown, not none). */
  loaded: boolean;
  profiles: Profile[];
  folders: Folder[];
  /** The order the sessions and folders are shown in (see `SessionTree`). */
  order: TreeItem[];
  proxies: Proxy[];
  /** Ids of the sessions opened recently, most recent first; deleted ones too. */
  recent: string[];
}

export const INITIAL_DIRECTORY: DirectoryState = { loaded: false, profiles: [], folders: [], order: [], proxies: [], recent: [] };

export type DirectoryAction =
  | { type: "loaded"; profiles: Profile[]; folders: Folder[]; order: TreeItem[]; proxies: Proxy[] }
  /** One session saved here, known without reading them all again (its port forwards). */
  | { type: "profileSaved"; profile: Profile }
  | { type: "recent"; ids: string[] }
  | { type: "reset" };

export function reduceDirectory(state: DirectoryState, action: DirectoryAction): DirectoryState {
  switch (action.type) {
    case "loaded":
      return { ...state, loaded: true, profiles: action.profiles, folders: action.folders, order: action.order, proxies: action.proxies };
    case "profileSaved":
      if (!state.profiles.some((p) => p.id === action.profile.id)) return state;
      return { ...state, profiles: state.profiles.map((p) => (p.id === action.profile.id ? action.profile : p)) };
    case "recent":
      return action.ids.length === state.recent.length && action.ids.every((id, i) => id === state.recent[i])
        ? state
        : { ...state, recent: action.ids };
    case "reset":
      return state === INITIAL_DIRECTORY ? state : INITIAL_DIRECTORY;
  }
}

/** The session a pane connects to; undefined for other panes and for deleted sessions. */
export function paneProfile(profiles: Profile[], pane: Pane): Profile | undefined {
  const target = pane.target;
  return target.kind === "profile" ? profiles.find((p) => p.id === target.profileId) : undefined;
}

/** The recent sessions to show, most recent first, without deleted ones. */
export const recentProfiles = (state: DirectoryState): Profile[] =>
  state.recent
    .map((id) => state.profiles.find((p) => p.id === id))
    .filter((p): p is Profile => !!p)
    .slice(0, RECENT_LIMIT);

export interface DirectoryStore {
  /** The current state, also between a change and the next render. */
  get(): DirectoryState;
  dispatch(action: DirectoryAction): void;
  subscribe(listener: () => void): () => void;
  /**
   * Reads the sessions, folders and proxies again: after a change made here, which the other
   * windows hear of (`config-changed`) but this one doesn't. A reply older than the latest
   * request is dropped. Never fails; errors are logged.
   */
  reload(): Promise<void>;
  /** Puts a session first among the recent ones, which all windows share. */
  opened(profileId: string): void;
  /** Reads the recent sessions again (another window opened one). */
  recentChanged(): void;
  /** Forgets everything, and any reply still to come (the app is unmounted: the vault locked). */
  clear(): void;
}

export function createDirectoryStore(initial: DirectoryState = INITIAL_DIRECTORY): DirectoryStore {
  let state = initial;
  let latest = 0;
  const listeners = new Set<() => void>();
  const dispatch = (action: DirectoryAction) => {
    const next = reduceDirectory(state, action);
    if (next === state) return;
    state = next;
    for (const listener of listeners) listener();
  };
  return {
    get: () => state,
    dispatch,
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    async reload() {
      const request = ++latest;
      try {
        const [profiles, folders, order, proxies] = await Promise.all([listProfiles(), tree.folders(), tree.order(), proxyApi.list()]);
        if (request === latest) dispatch({ type: "loaded", profiles, folders, order, proxies });
      } catch (e) {
        console.error(e);
      }
    },
    opened: (profileId) => dispatch({ type: "recent", ids: addRecent(profileId) }),
    recentChanged: () => dispatch({ type: "recent", ids: storedRecent() }),
    clear() {
      latest++;
      dispatch({ type: "reset" });
    },
  };
}

/** This window's session directory, kept up to date by `useDirectorySync` while the app is shown. */
export const directory = createDirectoryStore();
