/** A value a text field suggests (`ComboBox`), with what it is when the value doesn't say (a serial port's adapter). */
export interface Suggestion {
  value: string;
  description?: string;
}

/** The suggestions whose value or description contains `text`, ignoring case, in their order; all of them for no text. */
export function matchSuggestions(suggestions: readonly Suggestion[], text: string): readonly Suggestion[] {
  const needle = text.trim().toLowerCase();
  if (needle === "") return suggestions;
  return suggestions.filter((s) => s.value.toLowerCase().includes(needle) || !!s.description?.toLowerCase().includes(needle));
}
