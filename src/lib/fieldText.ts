/**
 * What a field showing a stored value shows once the value changes, here or elsewhere (in
 * another window): `current` is its text, `previous` the value it last got. The new `value`
 * replaces the text, unless the text was edited since `previous` came and not saved yet: the
 * edit stays, and replaces the value when it is saved. `same`: whether a text shows a value.
 */
export function nextFieldText(
  current: string,
  previous: string,
  value: string,
  same: (text: string, value: string) => boolean = (text, value) => text === value,
): string {
  if (same(current, value)) return current;
  return same(current, previous) ? value : current;
}
