import { useEffect, useEffectEvent, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

import { continuesTypeAhead, typeAhead, type TypeAhead } from "../lib/listNavigation";
import { CheckIcon, ChevronIcon, DotIcon } from "./icons";

/** An item that runs something when chosen. */
export interface ActionItem {
  label: string;
  /** Shortcut shown on the right, e.g. "⌘C". */
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  /** For a choice among several: shows a check mark when true, room for one when false. */
  checked?: boolean;
  /** Marks the item with a dot (something new there), which this text names for screen readers. */
  badge?: string;
  onSelect(): void;
}

/** An item that opens a submenu. */
export interface SubmenuItem {
  label: string;
  disabled?: boolean;
  badge?: string;
  submenu: MenuItem[];
}

export type MenuItem = ActionItem | SubmenuItem | "separator";

const isSubmenu = (item: ActionItem | SubmenuItem): item is SubmenuItem => "submenu" in item;
const choosable = (item: MenuItem): item is ActionItem | SubmenuItem => item !== "separator" && !item.disabled;

interface Props {
  /** Where the menu opens (viewport coordinates); it is moved to stay inside the window. */
  x: number;
  y: number;
  items: MenuItem[];
  /** Called when the menu closes, whether or not an item was chosen (before `onSelect`). */
  onClose(): void;
  /** The button that opened the menu: pressing it is left to the button, which closes the menu. */
  anchor?: HTMLElement | null;
  /** Highlights the first item: the menu was opened from the keyboard. */
  highlightFirst?: boolean;
}

const MARGIN = 4;
/** How long the pointer rests on an item before its submenu opens, or another one closes. */
const SUBMENU_DELAY = 200;

/**
 * A context menu drawn in the page, so it follows the theme on both platforms. Closes on
 * Escape, a click elsewhere, or when the window loses focus; arrow keys, Home and End move,
 * Enter or Space choose. The menu keeps the focus and points screen readers at the
 * highlighted item; once closed, the focus goes back where it was, before the chosen item
 * acts.
 *
 * Items may open submenus, beside them: from the keyboard with → (or Enter), back with ← or
 * Escape; with the pointer after it rests on the item, so that moving across other items
 * on the way to a submenu doesn't switch it.
 *
 * A menu taller than the window scrolls, keeping the item the keys highlight in sight, and typing
 * the start of an item's label highlights it (a long list of tabs).
 */
export function ContextMenu({ x, y, items, onClose, anchor, highlightFirst }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [opener] = useState(() => document.activeElement);
  const onCloseEvent = useEffectEvent(onClose);

  useEffect(() => {
    const close = () => onCloseEvent();
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!ref.current?.contains(target) && !anchor?.contains(target)) close();
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
  }, [opener, anchor]);

  // The focus goes back first, so the item acts as if the menu had not been there: on the
  // terminal it was opened from, and an item that moves the focus (a new pane, a dialog) can.
  const choose = (item: ActionItem) => {
    onClose();
    if (opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
    item.onSelect();
  };

  // The menu and its open submenus, each a `role="menu"` of its own.
  return createPortal(
    <div ref={ref}>
      <MenuPanel items={items} at={{ x, y }} focus highlightFirst={highlightFirst} onChoose={choose} onCloseAll={onClose} />
    </div>,
    document.body,
  );
}

interface PanelProps {
  items: MenuItem[];
  /** A point (the menu), or the item a submenu belongs to: it opens to its right, else its left. */
  at: { x: number; y: number } | { beside: HTMLElement };
  /** Takes the keyboard focus when it opens: the menu, or a submenu opened from the keyboard. */
  focus: boolean;
  highlightFirst?: boolean;
  /** A submenu's id, and its item's. */
  id?: string;
  labelledBy?: string;
  onChoose(item: ActionItem): void;
  onCloseAll(): void;
  /** A submenu's: closes it, back to its item. */
  onBack?(): void;
  /** A submenu's: the pointer came into it. */
  onPointerIn?(): void;
}

/** One level of a menu. */
function MenuPanel({ items, at, focus, highlightFirst, id, labelledBy, onChoose, onCloseAll, onBack, onPointerIn }: PanelProps) {
  const ownId = useId();
  const panelId = id ?? ownId;
  const itemId = (index: number) => `${panelId}-${index}`;
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState(() => ("beside" in at ? { left: -9999, top: 0 } : { left: at.x, top: at.y }));
  // Index into `items` of the highlighted entry, or -1.
  const [highlight, setHighlight] = useState(() => (highlightFirst ? items.findIndex(choosable) : -1));
  // The submenu shown, its item, and whether it took the focus.
  const [open, setOpen] = useState<{ index: number; item: HTMLElement; focus: boolean } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);
  /** What was typed to find an item, and when last. */
  const typed = useRef<TypeAhead>({ text: "", at: 0 });

  const x = "beside" in at ? null : at.x;
  const y = "beside" in at ? null : at.y;
  const beside = "beside" in at ? at.beside : null;
  useLayoutEffect(() => {
    const menu = ref.current!;
    const { width, height } = menu.getBoundingClientRect();
    if (beside) {
      const item = beside.getBoundingClientRect();
      const right = item.right + MARGIN;
      const left = right + width <= window.innerWidth - MARGIN ? right : item.left - MARGIN - width;
      // Its first item level with the one it belongs to, past its padding and border.
      const first = menu.firstElementChild?.getBoundingClientRect().top ?? 0;
      const top = item.top - (first - menu.getBoundingClientRect().top);
      setPosition({
        left: Math.max(MARGIN, left),
        top: Math.max(MARGIN, Math.min(top, window.innerHeight - height - MARGIN)),
      });
    } else {
      setPosition({
        left: Math.max(MARGIN, Math.min(x!, window.innerWidth - width - MARGIN)),
        top: Math.max(MARGIN, Math.min(y!, window.innerHeight - height - MARGIN)),
      });
    }
  }, [x, y, beside]);

  // Keyboard navigation, and keeps keys away from the terminal while open. A submenu shown on
  // hover takes it when entered from the keyboard.
  useLayoutEffect(() => {
    if (!focus) return;
    ref.current!.focus({ preventScroll: true });
    if (highlightFirst) setHighlight((current) => (current >= 0 ? current : items.findIndex(choosable)));
    // Only when it starts to have the focus.
  }, [focus]);

  useEffect(() => () => clearTimeout(timer.current), []);

  const later = (f: () => void) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(f, SUBMENU_DELAY);
  };

  const show = (index: number, focus: boolean) => {
    const item = itemRefs.current[index];
    if (item) setOpen({ index, item, focus });
  };

  const openSubmenu = (index: number, focus: boolean) => {
    clearTimeout(timer.current);
    setHighlight(index);
    show(index, focus);
  };

  const choose = (index: number, fromKeyboard: boolean) => {
    const item = items[index];
    if (!choosable(item)) return;
    if (isSubmenu(item)) openSubmenu(index, fromKeyboard);
    else onChoose(item);
  };

  /**
   * Highlights the next item that can be chosen, `step` items on from `from`, from the keyboard,
   * and scrolls it into sight if need be; a submenu shown (on hover) closes as its item loses
   * the highlight. Not as the pointer highlights an item: one only partly in sight would scroll
   * under it.
   */
  const move = (step: number, from = highlight) => {
    const count = items.length;
    let index = from;
    for (let i = 0; i < count; i++) {
      index = (index + step + count) % count;
      if (choosable(items[index])) {
        clearTimeout(timer.current);
        setHighlight(index);
        itemRefs.current[index]?.scrollIntoView({ block: "nearest" });
        if (open && open.index !== index) setOpen(null);
        return;
      }
    }
  };

  /** Highlights the item that what was typed finds (see `typeAhead`). */
  const findTyped = (key: string) => {
    const labels = items.map((item) => (choosable(item) ? item.label : null));
    const found = typeAhead(typed.current, key, Date.now(), labels, highlight);
    typed.current = found.typed;
    if (found.index >= 0) move(0, found.index);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    e.stopPropagation();
    // A space continues a prefix being typed; otherwise it chooses.
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && (e.key !== " " || continuesTypeAhead(typed.current, Date.now()))) {
      findTyped(e.key);
      e.preventDefault();
      return;
    }
    const item = highlight >= 0 ? items[highlight] : null;
    const submenu = item !== null && choosable(item) && isSubmenu(item);
    if (e.key === "Escape") (onBack ?? onCloseAll)();
    else if (e.key === "Tab") onCloseAll();
    else if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Home") move(1, -1);
    else if (e.key === "End") move(-1, items.length);
    else if (e.key === "ArrowRight" && submenu) openSubmenu(highlight, true);
    else if (e.key === "ArrowLeft" && onBack) onBack();
    else if ((e.key === "Enter" || e.key === " ") && highlight >= 0) choose(highlight, true);
    else return;
    e.preventDefault();
  };

  const onItemEnter = (index: number) => {
    const item = items[index];
    if (!choosable(item)) {
      setHighlight(-1);
      if (open) later(() => setOpen(null));
      return;
    }
    setHighlight(index);
    if (open?.index === index) clearTimeout(timer.current);
    else if (isSubmenu(item)) later(() => show(index, false));
    else if (open) later(() => setOpen(null));
  };

  const back = () => {
    setOpen(null);
    ref.current?.focus({ preventScroll: true });
  };

  const openItem = open && items[open.index];
  return (
    <>
      <div
        ref={ref}
        id={panelId}
        className="context-menu"
        role="menu"
        tabIndex={-1}
        aria-labelledby={labelledBy}
        aria-activedescendant={highlight >= 0 ? itemId(highlight) : undefined}
        style={position}
        onKeyDown={onKeyDown}
        onContextMenu={(e) => e.preventDefault()}
        onMouseEnter={() => {
          onPointerIn?.();
          // The keys follow the pointer between a menu and its submenu.
          if (document.activeElement !== ref.current) ref.current?.focus({ preventScroll: true });
        }}
        onMouseLeave={() => {
          // A submenu about to open or close stays as it is once the pointer is gone.
          clearTimeout(timer.current);
          setHighlight(open ? open.index : -1);
        }}
      >
        {items.map((item, index) =>
          item === "separator" ? (
            <div key={index} className="context-menu-separator" role="separator" />
          ) : (
            <div
              key={index}
              ref={(element) => void (itemRefs.current[index] = element)}
              id={itemId(index)}
              role={!isSubmenu(item) && item.checked !== undefined ? "menuitemradio" : "menuitem"}
              aria-checked={isSubmenu(item) ? undefined : item.checked}
              aria-disabled={item.disabled}
              aria-haspopup={isSubmenu(item) ? "menu" : undefined}
              aria-expanded={isSubmenu(item) ? open?.index === index : undefined}
              aria-controls={isSubmenu(item) && open?.index === index ? `${panelId}-sub` : undefined}
              className={`context-menu-item${index === highlight ? " highlighted" : ""}${!isSubmenu(item) && item.danger ? " danger" : ""}`}
              onMouseEnter={() => onItemEnter(index)}
              onClick={() => choose(index, false)}
            >
              {!isSubmenu(item) && item.checked !== undefined && (
                <span className="context-menu-check" aria-hidden="true">
                  {item.checked && <CheckIcon />}
                </span>
              )}
              <span className="context-menu-label">{item.label}</span>
              {item.badge && (
                <span className="context-menu-badge">
                  <DotIcon />
                  <span className="visually-hidden">{item.badge}</span>
                </span>
              )}
              {!isSubmenu(item) && item.shortcut && <kbd aria-hidden="true">{item.shortcut}</kbd>}
              {isSubmenu(item) && (
                <span className="context-menu-arrow">
                  <ChevronIcon direction="right" inline={false} />
                </span>
              )}
            </div>
          ),
        )}
      </div>
      {openItem && openItem !== "separator" && isSubmenu(openItem) && (
        <MenuPanel
          key={open.index}
          items={openItem.submenu}
          at={{ beside: open.item }}
          focus={open.focus}
          highlightFirst={open.focus}
          id={`${panelId}-sub`}
          labelledBy={itemId(open.index)}
          onChoose={onChoose}
          onCloseAll={onCloseAll}
          onBack={back}
          onPointerIn={() => {
            clearTimeout(timer.current);
            setHighlight(open!.index);
          }}
        />
      )}
    </>
  );
}
