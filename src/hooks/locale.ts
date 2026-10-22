import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { currentCjk } from "../i18n/index";
import { compareNames, formatAgo, formatList, formatSize, formatTime } from "../lib/format";
import { shortcutLabel, shortcutRows, type Action } from "../lib/keymap";

/**
 * The helpers whose text depends on the interface language, for rendering. They read state the
 * language sets outside React (the key names, `i18n.language`), which React Compiler can't see:
 * a memoized render would keep showing the previous language's. Taken from here they are new
 * functions whenever the language changes, so what they produced is worked out again. Event
 * handlers and code outside React can call the ones in `lib/` (`locale.test.ts` checks).
 */
export function useLocale() {
  const { i18n } = useTranslation();
  const language = i18n.language;
  return useMemo(
    () => ({
      shortcutLabel: (action: Action) => shortcutLabel(action),
      shortcutRows: () => shortcutRows(),
      formatSize: (bytes: number) => formatSize(bytes),
      formatTime: (seconds: number | null) => formatTime(seconds),
      formatAgo: (ms: number) => formatAgo(ms),
      compareNames: (a: string, b: string) => compareNames(a, b),
      formatList: (items: string[], style?: "long" | "narrow") => formatList(items, style),
      /** The CJK language whose fonts come first (see `currentCjk`). */
      cjk: currentCjk(),
    }),
    // The functions above read the language through module state.
    [language],
  );
}
