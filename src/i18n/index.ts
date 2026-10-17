import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import { locales } from "../ipc/api";
import { LANGUAGES, type Cjk, type Locales } from "../ipc/bindings";
import { KEY_NAMES, setKeyNames, type KeyName } from "../lib/keymap";
import en from "../locales/en.json";

export type Language = (typeof LANGUAGES)[number];

/**
 * UI catalogs, one for each of the backend's. English is the source language and the fallback
 * for missing keys, always there; the others are loaded once needed, as each window would
 * otherwise parse all of them as it starts.
 */
const catalogs = {
  en: async () => en,
  "zh-Hans": async () => (await import("../locales/zh-Hans.json")).default,
  ja: async () => (await import("../locales/ja.json")).default,
  de: async () => (await import("../locales/de.json")).default,
  fr: async () => (await import("../locales/fr.json")).default,
} satisfies Record<Language, () => Promise<object>>;

/** Makes the catalog of `language` available, if it isn't yet. */
async function loadCatalog(language: Language) {
  if (i18n.hasResourceBundle(language, "translation")) return;
  i18n.addResourceBundle(language, "translation", await catalogs[language]());
}

export const isLanguage = (value: string): value is Language => (LANGUAGES as readonly string[]).includes(value);

/** The system's language and CJK language (see `Locales` in i18n.rs), read once. */
let system: Locales = { current: "en", system: "en", cjk: "zh-Hans" };

/** The language the `language` setting chooses: its own, or the system's. */
export const languageFor = (setting: string): Language =>
  isLanguage(setting) ? setting : isLanguage(system.system) ? system.system : "en";

/** The system's language, which the "system" setting chooses. */
export const systemLanguage = () => languageFor("system");

/** Starts in the language the backend chose from the settings. */
export async function setupI18n() {
  system = await locales().catch((e) => {
    console.error("failed to get the locales", e);
    return system;
  });
  const language = isLanguage(system.current) ? system.current : "en";
  const catalog = await catalogs[language]().catch((e: unknown) => {
    console.error("failed to load the catalog", e);
    return en;
  });
  await i18n.use(initReactI18next).init({
    resources: { en: { translation: en }, [language]: { translation: catalog } },
    lng: language,
    fallbackLng: "en",
    supportedLngs: LANGUAGES,
    // Same placeholder syntax as the backend catalogs in src-tauri/locales.
    interpolation: { escapeValue: false, prefix: "{", suffix: "}" },
  });
  applyLanguage(language);
}

/** Counts the languages chosen, so that only the latest is switched to. */
let languageRequest = 0;

/**
 * Switches to the language the `language` setting chooses, once it changes here or in another
 * window. The backend switches itself when the settings are saved.
 */
export async function followLanguageSetting(setting: string) {
  const language = languageFor(setting);
  const request = ++languageRequest;
  await loadCatalog(language);
  // Chosen again since, while the catalog loaded: the newer choice decides.
  if (request !== languageRequest || language === i18n.language) return;
  applyLanguage(language);
  await i18n.changeLanguage(language);
}

/**
 * The document's language, which selects hyphenation and localized glyphs; the CJK fonts that
 * come first (`data-cjk`, see styles/theme.css): the interface's if it is in one, otherwise the
 * system's first; and the names of keys in shortcuts. Before the page renders in it.
 */
function applyLanguage(language: Language) {
  const root = document.documentElement;
  root.lang = language;
  root.dataset.cjk = cjkFor(language);
  const t = i18n.getFixedT(language);
  setKeyNames(Object.fromEntries(KEY_NAMES.map((name) => [name, t(`keys.${name}`)])) as Record<KeyName, string>);
}

const CJK: readonly string[] = ["zh-Hans", "zh-Hant", "ja", "ko"] satisfies Cjk[];

const cjkFor = (language: string): Cjk => (CJK.includes(language) ? (language as Cjk) : system.cjk);

/** The CJK language whose fonts come first, as `applyLanguage` sets it. */
export const currentCjk = () => cjkFor(i18n.language);

export default i18n;
