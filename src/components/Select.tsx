import { useId, useRef, useState, type KeyboardEvent } from "react";

import { clampIndex, continuesTypeAhead, typeAhead, type TypeAhead } from "../lib/listNavigation";
import { ChevronIcon } from "./icons";
import { ListPopup, optionId } from "./ListPopup";

export interface SelectOption<T> {
  value: T;
  label: string;
  /** The language the label is in, when it isn't the interface's (a language's own name). */
  lang?: string;
}

interface Props<T> {
  value: T;
  options: readonly SelectOption<T>[];
  /** Only when another option is chosen. */
  onChange(value: NoInfer<T>): void;
  disabled?: boolean;
  /** Shown when no option has the value: what choosing one does ("Add a jump host…"). */
  placeholder?: string;
  /** Names it when no `<label>` is around it. */
  "aria-label"?: string;
}

/** How many options PageUp and PageDown move. */
const PAGE = 10;

/**
 * A drop-down list, in place of `<select>`: a button that shows the option chosen and opens
 * the options in the page (`ListPopup`), so that they look the same on every platform and
 * follow the theme. A select-only combobox (as in the ARIA patterns): the button keeps the
 * focus; ↓, ↑, Enter or Space open the list at the option chosen, and so does typing the start
 * of an option. Open, the arrow keys, Home, End, PageUp and PageDown move, typing finds an
 * option, Enter or Space choose, and Escape or Tab close without choosing.
 *
 * Inside a `<label>`, which names it; a click on the label's text opens or closes it, as a click
 * on a checkbox's text checks it.
 */
export function Select<T extends string | number>({ value, options, onChange, disabled, placeholder, "aria-label": ariaLabel }: Props<T>) {
  const id = useId();
  const listId = `${id}-list`;
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const typed = useRef<TypeAhead>({ text: "", at: 0 });
  const chosen = options.findIndex((option) => option.value === value);
  const shown = open && !disabled;

  const show = (index: number) => {
    setHighlight(clampIndex(index, options.length));
    setOpen(true);
  };
  const pick = (index: number) => {
    setOpen(false);
    const option = options[index];
    if (option && option.value !== value) onChange(option.value);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    const now = Date.now();
    // A space continues a prefix being typed; otherwise it opens or chooses.
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && (e.key !== " " || continuesTypeAhead(typed.current, now))) {
      const labels = options.map((option) => option.label);
      const found = typeAhead(typed.current, e.key, now, labels, shown ? highlight : chosen);
      typed.current = found.typed;
      if (found.index >= 0) show(found.index);
      else if (!shown) show(chosen);
      e.preventDefault();
      return;
    }
    if (!shown) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") show(chosen);
      else return;
    } else if (e.key === "Escape") setOpen(false);
    else if (e.key === "Tab") {
      setOpen(false);
      return;
    } else if (e.key === "ArrowUp" && e.altKey) pick(highlight);
    else if (e.key === "ArrowDown") setHighlight(clampIndex(highlight + 1, options.length));
    else if (e.key === "ArrowUp") setHighlight(clampIndex(highlight - 1, options.length));
    else if (e.key === "PageDown") setHighlight(clampIndex(highlight + PAGE, options.length));
    else if (e.key === "PageUp") setHighlight(clampIndex(highlight - PAGE, options.length));
    else if (e.key === "Home") setHighlight(clampIndex(0, options.length));
    else if (e.key === "End") setHighlight(options.length - 1);
    else if (e.key === "Enter" || e.key === " ") pick(highlight);
    else return;
    // Escape closes the list rather than the dialog; Enter and Space don't click the button.
    e.preventDefault();
  };

  const option = chosen >= 0 ? options[chosen] : null;
  return (
    <>
      <button
        ref={button}
        type="button"
        className="select"
        role="combobox"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={shown}
        aria-controls={shown ? listId : undefined}
        aria-activedescendant={shown && highlight >= 0 ? optionId(listId, highlight) : undefined}
        id={id}
        disabled={disabled}
        // WebKit on macOS doesn't focus a button that is pressed but the element around it,
        // which would close the list as the button lost the focus: it is focused on the click.
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          button.current?.focus();
          if (shown) setOpen(false);
          else show(chosen);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => setOpen(false)}
      >
        {option ? (
          <span className="select-value" lang={option.lang}>
            {option.label}
          </span>
        ) : (
          <span className="select-value select-placeholder">{placeholder}</span>
        )}
        <ChevronIcon direction="down" inline={false} />
      </button>
      {shown && (
        <ListPopup
          id={listId}
          anchor={button}
          labelledBy={id}
          items={options}
          highlight={highlight}
          selected={chosen}
          onHighlight={setHighlight}
          onPick={pick}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}
