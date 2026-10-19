import { appSettings } from "../ipc/api";
import { cachedRanges, fontLigatures } from "./fontLigatures";
import { fontFamilies, ligatureRanges } from "./ligatures";

/**
 * Where ligatures form by a font's own rules (its substitutions), for the "full" ligatures
 * setting. The backend reads the rules from the installed font (`fonts/` there), and
 * `fontLigatures.ts` applies them. The terminals with the same font share them while any uses
 * them, and the results for the lines drawn last, which come back as the screen scrolls.
 */

type Ranges = (text: string) => [number, number][];

interface Shared {
  /** The terminals using the rules. */
  users: number;
  /** The font's rules once read; null before, or if they can't be. */
  find: Ranges | null;
  /** Told once the rules are read, to draw again. */
  loaded: Set<() => void>;
}

/** By font list. */
const fonts = new Map<string, Shared>();

/** Cached results of the font's rules, in characters of text (about 650 KB). */
const CACHE_SIZE = 100_000;

async function load(fontFamily: string): Promise<Ranges | null> {
  const rules = await appSettings.ligatureRules(fontFamilies(fontFamily));
  if (!rules) return null;
  const font = fontLigatures(rules);
  return cachedRanges((text) => font.ranges(text), CACHE_SIZE);
}

/**
 * Where ligatures form in a terminal drawn with `fontFamily` (a CSS font list, its first
 * installed font being the one drawn with), until released. The usual sequences
 * (`ligatureRanges`) stand in until the font's rules are read, and if they can't be;
 * `loaded` is called once they are, to draw the terminal again.
 */
export function fontLigatureRules(fontFamily: string, loaded: () => void): { ranges: Ranges; release(): void } {
  let shared = fonts.get(fontFamily);
  if (!shared) {
    const created: Shared = { users: 0, find: null, loaded: new Set() };
    shared = created;
    fonts.set(fontFamily, created);
    load(fontFamily)
      .catch((e) => {
        console.warn("can't read the font's ligature rules, using the usual sequences", e);
        return null;
      })
      .then((find) => {
        created.find = find;
        if (find) created.loaded.forEach((listener) => listener());
      });
  }
  const own = shared;
  own.users++;
  own.loaded.add(loaded);
  let released = false;
  return {
    ranges: (text) => (own.find ?? ligatureRanges)(text),
    release() {
      if (released) return;
      released = true;
      own.loaded.delete(loaded);
      if (--own.users === 0 && fonts.get(fontFamily) === own) fonts.delete(fontFamily);
    },
  };
}
