import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import { LANGUAGES, type Cjk, type Locales } from "../ipc/bindings";
import { KEY_NAMES, setKeyNames, type KeyName } from "../lib/keymap";
import de from "../locales/de.json";
import en from "../locales/en.json";
import fr from "../locales/fr.json";
import ja from "../locales/ja.json";
import zhHans from "../locales/zh-Hans.json";

export type Language = (typeof LANGUAGES)[number];

/** UI catalogs, one for each of the backend's. English is the source language and the fallback for missing keys. */
export const resources = {
  en: { translation: en },
  "zh-Hans": { translation: zhHans },
  ja: { translation: ja },
  de: { translation: de },
  fr: { translation: fr },
} as const satisfies Record<Language, { translation: object }>;

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
  system = await invoke<Locales>("locale_get").catch((e) => {
    console.error("failed to get the locales", e);
    return system;
  });
  const language = isLanguage(system.current) ? system.current : "en";
  await i18n.use(initReactI18next).init({
    resources,
    lng: language,
    fallbackLng: "en",
    supportedLngs: LANGUAGES,
    // Same placeholder syntax as the backend catalogs in src-tauri/locales.
    interpolation: { escapeValue: false, prefix: "{", suffix: "}" },
  });
  applyLanguage(language);
}

/**
 * Switches to the language the `language` setting chooses, once it changes here or in another
 * window. The backend switches itself when the settings are saved.
 */
export async function followLanguageSetting(setting: string) {
  const language = languageFor(setting);
  if (language === i18n.language) return;
  applyLanguage(language);
  await i18n.changeLanguage(language);
}

/**
 * The document's language, which selects hyphenation and localized glyphs; the CJK fonts that
 * come first (`data-cjk`, see styles.css): the interface's if it is in one, otherwise the
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
