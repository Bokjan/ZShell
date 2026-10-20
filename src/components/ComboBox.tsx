import { useId, useRef, useState, type InputHTMLAttributes, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { clampIndex } from "../lib/listNavigation";
import { matchSuggestions, type Suggestion } from "../lib/suggestions";
import { isComposing } from "../lib/platform";
import { ChevronIcon } from "./icons";
import { ListPopup, optionId } from "./ListPopup";

type Props = Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "list"> & {
  value: string;
  onChange(value: string): void;
  suggestions: readonly Suggestion[];
};

/**
 * A text field with suggestions, in place of `<input list>`: any text can be typed, and a list
 * in the page (`ListPopup`) offers values. The `<datalist>` popup only offers values that
 * contain the text already there, so a field showing one of them offers nothing else; it also
 * looks different on every platform. Here the button at the right end (or ↓, Alt+↓) shows all
 * of them, at the one in the field, and typing shows those that contain the text. ↑ / ↓ move,
 * Enter takes the highlighted one, Escape closes the list.
 */
export function ComboBox({ value, onChange, suggestions, onKeyDown, onBlur, id: givenId, ...rest }: Props) {
  const { t } = useTranslation();
  const ownId = useId();
  const id = givenId ?? ownId;
  const listId = `${id}-list`;
  const field = useRef<HTMLSpanElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // Open with all the suggestions (null) or those that match the text typed since.
  const [filter, setFilter] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const shown = open && !rest.disabled ? (filter === null ? suggestions : matchSuggestions(suggestions, filter)) : [];
  const current = shown.findIndex((suggestion) => suggestion.value === value);

  const showAll = () => {
    setFilter(null);
    setOpen(true);
    setHighlight(suggestions.findIndex((suggestion) => suggestion.value === value));
  };
  const close = () => {
    setOpen(false);
    setHighlight(-1);
  };
  const pick = (index: number) => {
    const suggestion = shown[index];
    close();
    if (suggestion) onChange(suggestion.value);
  };

  const keyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || isComposing(e)) return;
    if (shown.length === 0) {
      if (e.key === "ArrowDown") showAll();
      else return;
    } else if (e.key === "ArrowUp" && e.altKey) close();
    else if (e.key === "ArrowDown") setHighlight(clampIndex(highlight + 1, shown.length));
    else if (e.key === "ArrowUp") setHighlight(clampIndex(highlight - 1, shown.length));
    else if (e.key === "Enter" && highlight >= 0) pick(highlight);
    // Escape closes the list rather than the dialog.
    else if (e.key === "Escape") close();
    else {
      if (e.key === "Tab" || e.key === "Enter") close();
      return;
    }
    e.preventDefault();
  };

  return (
    <span ref={field} className="combo-box">
      <input
        {...rest}
        ref={input}
        id={id}
        value={value}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={shown.length > 0}
        aria-controls={shown.length > 0 ? listId : undefined}
        aria-activedescendant={shown.length > 0 && highlight >= 0 ? optionId(listId, highlight) : undefined}
        onChange={(e) => {
          onChange(e.target.value);
          setFilter(e.target.value);
          setOpen(true);
          setHighlight(-1);
        }}
        onKeyDown={keyDown}
        onBlur={(e) => {
          close();
          onBlur?.(e);
        }}
      />
      <button
        type="button"
        tabIndex={-1}
        aria-label={t("common.showSuggestions")}
        disabled={rest.disabled || suggestions.length === 0}
        // The field keeps the focus.
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          input.current?.focus();
          if (shown.length > 0 && filter === null) close();
          else showAll();
        }}
      >
        <ChevronIcon direction="down" inline={false} />
      </button>
      {shown.length > 0 && (
        <ListPopup
          id={listId}
          anchor={field}
          labelledBy={id}
          items={shown.map((suggestion) => ({ label: suggestion.value, detail: suggestion.description }))}
          highlight={highlight}
          selected={current}
          onHighlight={setHighlight}
          onPick={pick}
          onClose={close}
        />
      )}
    </span>
  );
}
