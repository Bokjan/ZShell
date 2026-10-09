import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

export type MenuItem =
  | {
      label: string;
      /** Shortcut shown on the right, e.g. "⌘C". */
      shortcut?: string;
      disabled?: boolean;
      danger?: boolean;
      /** For a choice among several: shows a check mark when true, room for one when false. */
      checked?: boolean;
      onSelect(): void;
    }
  | "separator";

interface Props {
  /** Where the menu opens (viewport coordinates); it is moved to stay inside the window. */
  x: number;
  y: number;
  items: MenuItem[];
  /** Called when the menu closes, whether or not an item was chosen (before `onSelect`). */
  onClose(): void;
}

const MARGIN = 4;

/**
 * A context menu drawn in the page, so it follows the theme on both platforms. Closes on
 * Escape, a click elsewhere, or when the window loses focus; arrow keys, Home and End move,
 * Enter or Space choose. The menu keeps the focus and points screen readers at the
 * highlighted item; once closed, the focus goes back where it was unless the chosen action
 * moved it.
 */
export function ContextMenu({ x, y, items, onClose }: Props) {
  const id = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [opener] = useState(() => document.activeElement);
  const [position, setPosition] = useState({ left: x, top: y });
  // Index into `items` of the highlighted entry, or -1.
  const [highlight, setHighlight] = useState(-1);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useLayoutEffect(() => {
    const menu = ref.current!;
    const { width, height } = menu.getBoundingClientRect();
    setPosition({
      left: Math.max(MARGIN, Math.min(x, window.innerWidth - width - MARGIN)),
      top: Math.max(MARGIN, Math.min(y, window.innerHeight - height - MARGIN)),
    });
    // Keyboard navigation, and keeps keys away from the terminal while open.
    menu.focus();
  }, [x, y]);

  useEffect(() => {
    const close = () => onCloseRef.current();
    const onMouseDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      requestAnimationFrame(() => {
        const lost = !document.activeElement || document.activeElement === document.body;
        if (lost && opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
      });
    };
  }, [opener]);

  const choose = (index: number) => {
    const item = items[index];
    if (item === "separator" || item.disabled) return;
    onClose();
    item.onSelect();
  };

  /** Highlights the next item that can be chosen, `step` items on from `from`. */
  const move = (step: number, from = highlight) => {
    const count = items.length;
    let index = from;
    for (let i = 0; i < count; i++) {
      index = (index + step + count) % count;
      const item = items[index];
      if (item !== "separator" && !item.disabled) {
        setHighlight(index);
        return;
      }
    }
  };

  const onKeyDown = (e: KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "Tab") onClose();
    else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Home") move(1, -1);
    else if (e.key === "End") move(-1, items.length);
    else if ((e.key === "Enter" || e.key === " ") && highlight >= 0) choose(highlight);
    else return;
    e.preventDefault();
  };

  return createPortal(
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      tabIndex={-1}
      aria-activedescendant={highlight >= 0 ? `${id}-${highlight}` : undefined}
      style={position}
      onKeyDown={onKeyDown}
      onContextMenu={(e) => e.preventDefault()}
      onMouseLeave={() => setHighlight(-1)}
    >
      {items.map((item, index) =>
        item === "separator" ? (
          <div key={index} className="context-menu-separator" role="separator" />
        ) : (
          <div
            key={index}
            id={`${id}-${index}`}
            role={item.checked === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.checked}
            aria-disabled={item.disabled}
            className={`context-menu-item${index === highlight ? " highlighted" : ""}${item.danger ? " danger" : ""}`}
            onMouseEnter={() => setHighlight(item.disabled ? -1 : index)}
            onClick={() => choose(index)}
          >
            {item.checked !== undefined && (
              <span className="context-menu-check" aria-hidden="true">
                {item.checked ? "✓" : ""}
              </span>
            )}
            <span className="context-menu-label">{item.label}</span>
            {item.shortcut && <kbd aria-hidden="true">{item.shortcut}</kbd>}
          </div>
        ),
      )}
    </div>,
    document.body,
  );
}
