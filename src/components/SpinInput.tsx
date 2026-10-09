import { useEffect, useRef, type InputHTMLAttributes, type KeyboardEvent, type PointerEvent } from "react";
import { useTranslation } from "react-i18next";

/** How long a held ▲ / ▼ waits before repeating, and how often it repeats. */
const REPEAT_DELAY = 400;
const REPEAT_INTERVAL = 60;

type Props = Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "min" | "max" | "step"> & {
  value: string;
  /** `stepped` when the change came from ▲ / ▼ or the arrow keys rather than typing. */
  onChange(text: string, stepped: boolean): void;
  min: number;
  max: number;
  step?: number;
  /** Where stepping starts when the field is empty or not a number. */
  start: number;
};

/**
 * A text field for a whole number, with ▲ / ▼ buttons inside its right end and ↑ / ↓ (with
 * Shift, ten steps) that step the value within range. Drawn rather than `type="number"`,
 * whose spin buttons differ between WebKit and WebView2 and ignore the theme.
 */
export function SpinInput({ value, onChange, min, max, step = 1, start, onKeyDown, ...rest }: Props) {
  const { t } = useTranslation();
  // The latest value, for a held button's repeats.
  const latest = useRef(value);
  latest.current = value;
  const repeat = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(repeat.current), []);

  const current = () => {
    const n = Number(latest.current);
    return latest.current.trim() !== "" && Number.isFinite(n) ? Math.round(n) : start;
  };
  /** Returns whether the value can go further this way. */
  const stepBy = (steps: number) => {
    const next = Math.min(max, Math.max(min, current() + steps * step));
    latest.current = String(next);
    onChange(latest.current, true);
    return steps > 0 ? next < max : next > min;
  };

  const keyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(e);
    if (e.defaultPrevented || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    stepBy((e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 10 : 1));
  };

  const stop = () => window.clearTimeout(repeat.current);
  // Steps once, then repeatedly while held, until the end of the range (where the button is
  // disabled and gets no more pointer events). The field keeps the focus.
  const press = (steps: number) => (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    window.addEventListener("pointerup", stop, { once: true });
    const again = (delay: number) => {
      repeat.current = window.setTimeout(() => {
        if (stepBy(steps)) again(REPEAT_INTERVAL);
      }, delay);
    };
    if (stepBy(steps)) again(REPEAT_DELAY);
  };

  const n = current();
  return (
    <span className="spin-input">
      <input {...rest} value={value} inputMode="numeric" onChange={(e) => onChange(e.target.value, false)} onKeyDown={keyDown} />
      <span className="spin-buttons">
        <button
          type="button"
          tabIndex={-1}
          aria-label={t("common.increase")}
          disabled={rest.disabled || n >= max}
          onPointerDown={press(1)}
          onPointerUp={stop}
          onPointerLeave={stop}
        >
          <svg width="8" height="5" viewBox="0 0 8 5" aria-hidden="true">
            <path d="M1 4l3-3 3 3" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
        <button
          type="button"
          tabIndex={-1}
          aria-label={t("common.decrease")}
          disabled={rest.disabled || n <= min}
          onPointerDown={press(-1)}
          onPointerUp={stop}
          onPointerLeave={stop}
        >
          <svg width="8" height="5" viewBox="0 0 8 5" aria-hidden="true">
            <path d="M1 1l3 3 3-3" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      </span>
    </span>
  );
}
