import { useRef, type KeyboardEvent, type ReactNode } from "react";

import { rovingTarget } from "../lib/listNavigation";

export interface RadioOption<T> {
  value: T;
  content: ReactNode;
}

interface Props<T> {
  /** The group's name for screen readers, usually the text shown beside it. */
  label: string;
  value: T;
  options: RadioOption<T>[];
  onChange(value: T): void;
  className?: string;
  /** Of each option's button; " on" is added to the chosen one's. */
  optionClassName?: string;
}

/**
 * One choice among a few, shown as buttons (a segmented control, the color scheme cards). It
 * behaves like native radio buttons: the group is one stop for Tab, at the chosen option, and
 * the arrow keys (and Home / End) choose another option and move the focus to it.
 */
export function RadioGroup<T>({ label, value, options, onChange, className, optionClassName }: Props<T>) {
  const groupRef = useRef<HTMLDivElement>(null);
  const chosen = options.findIndex((option) => option.value === value);

  const onKeyDown = (e: KeyboardEvent) => {
    const next = rovingTarget(e.key, Math.max(chosen, 0), options.length);
    if (next === null) return;
    e.preventDefault();
    onChange(options[next].value);
    groupRef.current?.querySelectorAll<HTMLElement>("[role=radio]")[next]?.focus();
  };

  return (
    <div className={className} role="radiogroup" aria-label={label} ref={groupRef} onKeyDown={onKeyDown}>
      {options.map((option, i) => {
        const on = i === chosen;
        return (
          <button
            key={String(option.value)}
            type="button"
            role="radio"
            aria-checked={on}
            // Tab reaches the chosen option, or the first while none is.
            tabIndex={on || (chosen < 0 && i === 0) ? 0 : -1}
            className={[optionClassName, on && "on"].filter(Boolean).join(" ") || undefined}
            onClick={() => onChange(option.value)}
          >
            {option.content}
          </button>
        );
      })}
    </div>
  );
}
