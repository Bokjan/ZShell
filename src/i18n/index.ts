import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import en from "../locales/en.json";

/** UI catalogs. English is the source language and the fallback for missing keys. */
export const resources = {
  en: { translation: en },
} as const;

export type Language = keyof typeof resources;

const supported = Object.keys(resources) as Language[];

/** Best supported match for the user's preferred languages: "zh-Hans-CN" → "zh-Hans" → "zh" → "en". */
function detectLanguage(): Language {
  for (const tag of navigator.languages ?? [navigator.language]) {
    const parts = tag.split(/[-_]/);
    while (parts.length > 0) {
      const candidate = parts.join("-").toLowerCase();
      const match = supported.find((lang) => lang.toLowerCase() === candidate);
      if (match) return match;
      parts.pop();
    }
  }
  return "en";
}

export async function setupI18n() {
  await i18n.use(initReactI18next).init({
    resources,
    lng: detectLanguage(),
    fallbackLng: "en",
    supportedLngs: supported,
    // Same placeholder syntax as the backend catalogs in src-tauri/locales.
    interpolation: { escapeValue: false, prefix: "{", suffix: "}" },
  });
  await applyLanguage(i18n.language);
}

export async function changeLanguage(language: Language) {
  await i18n.changeLanguage(language);
  await applyLanguage(language);
}

/** Keeps the document and the backend (terminal prompts, error messages) in the same language. */
async function applyLanguage(language: string) {
  document.documentElement.lang = language;
  try {
    await invoke("set_locale", { locale: language });
  } catch (e) {
    console.error("failed to set backend locale", e);
  }
}

export default i18n;
