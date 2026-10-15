import { useSyncExternalStore } from "react";

import { directory } from "../lib/directory";
import type { TabStore } from "../lib/tabs";

/** The tabs of `store`, rendering again when they change. */
export const useTabStore = (store: TabStore) => useSyncExternalStore(store.subscribe, store.get);

/** The session directory, rendering again when it changes. */
export const useDirectory = () => useSyncExternalStore(directory.subscribe, directory.get);
