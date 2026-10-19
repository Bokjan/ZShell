// The part of opentype.js 2.0, which has no types, that `index.ts` uses.
declare module "opentype.js" {
  export interface Font {
    tables: {
      gsub?: {
        features: { tag: string; feature: { lookupListIndexes: number[] } }[];
        /** `Lookup`s (`tables.ts`). */
        lookups: unknown[];
      };
    };
    charToGlyphIndex(char: string): number;
  }

  /** `lowMemory` reads each glyph only when it is used. */
  export function parse(buffer: ArrayBuffer, options?: { lowMemory?: boolean }): Font;
}
