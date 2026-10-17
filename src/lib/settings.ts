import { appSettings, DEFAULT_SETTINGS, type Settings } from "../ipc/api";
import { createSavedStore } from "./savedStore";

/**
 * This window's settings, read by `SettingsProvider` when the page starts and again when
 * another window saves them; components read them through `useSettings`.
 */
export const settingsStore = createSavedStore<Settings>({ store: appSettings.set, load: appSettings.get });

/** The settings as this window last read them, outside React; the defaults before then. */
export const currentSettings = (): Settings => settingsStore.get().value ?? DEFAULT_SETTINGS;
