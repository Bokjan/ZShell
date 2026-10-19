/**
 * Character sequences the terminal draws together, for fonts with programming ligatures.
 *
 * xterm.js draws each cell on its own, so a ligature never forms unless a character joiner
 * names the cells to draw as one piece of text. Both renderers then draw that text with the
 * font, which shapes it by its own rules (`calt`): a font with a ligature for the sequence
 * draws it, others draw the characters as they would apart. The joiner only groups cells; it
 * is called with each run of cells in the same colors and style, and not for the cells under
 * the cursor or a partial selection, which are drawn apart.
 */

/**
 * The usual programming ligatures: Iosevka's `calt` set, which `@xterm/addon-ligatures` and
 * VS Code fall back to when they can't read the font. A sequence outside it isn't joined even
 * if the font has a ligature for it.
 */
export const LIGATURE_SEQUENCES = (
  "<-- <--- <<- <- -> ->> --> ---> " +
  "<== <=== <<= <= => =>> ==> ===> >= >>= " +
  "<-> <--> <---> <----> <=> <==> <===> <====> :: ::: " +
  "<~~ </ </> /> ~~> == != /= ~= <> === !== !=== " +
  "<: := *= *+ <* <*> *> <| <|> |> +* =* =: :> " +
  "/* */ +++ <!-- <!---"
).split(" ");

/** The sequences by their first character, longest first. */
const byFirst = new Map<string, string[]>();
for (const sequence of [...LIGATURE_SEQUENCES].sort((a, b) => b.length - a.length)) {
  const first = sequence[0]!;
  byFirst.set(first, [...(byFirst.get(first) ?? []), sequence]);
}

/**
 * The parts of `text` to draw together, as [start, end) offsets: the longest sequence at each
 * place, from the left, without overlapping (`<!---` in `<!----`, then `-` alone).
 */
export function ligatureRanges(text: string): [number, number][] {
  const ranges: [number, number][] = [];
  for (let i = 0; i < text.length; i++) {
    const sequences = byFirst.get(text[i]!);
    if (!sequences) continue;
    const sequence = sequences.find((s) => text.startsWith(s, i));
    if (!sequence) continue;
    ranges.push([i, i + sequence.length]);
    i += sequence.length - 1;
  }
  return ranges;
}
