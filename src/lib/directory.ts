import { listProfiles, proxies as proxyApi, tree, type Folder, type Profile, type Proxy, type TreeItem } from "../ipc/api";
import { RECENT_LIMIT, addRecent, storedRecent } from "./sessions";
import { createStore, type ReducerStore } from "./store";

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

/** The recent sessions to show, most recent first, without deleted ones. */
export const recentProfiles = (state: DirectoryState): Profile[] =>
  state.recent
    .map((id) => state.profiles.find((p) => p.id === id))
    .filter((p): p is Profile => !!p)
    .slice(0, RECENT_LIMIT);

export interface DirectoryStore extends ReducerStore<DirectoryState, DirectoryAction> {
  /**
   * Reads the sessions, folders and proxies again (another window changed them). A reply
   * older than the latest request or change is dropped. Never fails; errors are logged.
   */
  reload(): Promise<void>;
  /**
   * `write`, a change made here to the sessions, folders or proxies, which the other windows
   * hear of (`config-changed`) but this one doesn't: reads them again once it is done, and
   * resolves (or fails) as it does.
   */
  changed<T>(write: Promise<T>): Promise<T>;
  /** One session saved here, known without reading them all again (its port forwards). */
  profileSaved(profile: Profile): void;
  /** Puts a session first among the recent ones, which all windows share. */
  opened(profileId: string): void;
  /** Reads the recent sessions again (another window opened one). */
  recentChanged(): void;
  /** Forgets everything, and any reply still to come (the app is unmounted: the vault locked). */
  clear(): void;
}

export function createDirectoryStore(initial: DirectoryState = INITIAL_DIRECTORY): DirectoryStore {
  const store = createStore(initial, reduceDirectory);
  const { dispatch } = store;
  let latest = 0;
  // Reads on their way, whose replies a change here drops.
  let reading = 0;
  const reload = async () => {
    const request = ++latest;
    reading++;
    try {
      const [profiles, folders, order, proxies] = await Promise.all([listProfiles(), tree.folders(), tree.order(), proxyApi.list()]);
      if (request === latest) dispatch({ type: "loaded", profiles, folders, order, proxies });
    } catch (e) {
      console.error(e);
    } finally {
      reading--;
    }
  };
  return {
    ...store,
    reload,
    async changed(write) {
      const value = await write;
      void reload();
      return value;
    },
    profileSaved(profile) {
      // A reply on its way was read before this was saved, so it is dropped; what it was read
      // for (a change in another window) is read again.
      latest++;
      dispatch({ type: "profileSaved", profile });
      if (reading > 0) void reload();
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
