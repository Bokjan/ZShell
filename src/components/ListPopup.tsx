import { useEffect, useEffectEvent, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";

import { CheckIcon } from "./icons";

export interface ListPopupItem {
  label: string;
  /** Shown after the label, dimmer: a serial port's description. */
  detail?: string;
  /** The language the label is in, when it isn't the interface's (a language's own name). */
  lang?: string;
}

interface Props {
  /** The listbox's; each option's is `${id}-${index}`, for `aria-activedescendant`. */
  id: string;
  /** The field it drops from: placed below it (above when there is no room), at least as wide. */
  anchor: RefObject<HTMLElement | null>;
  /** The element that names the list: the field, which its label names. */
  labelledBy: string;
  items: readonly ListPopupItem[];
  /** The option the keys move from, or -1. */
  highlight: number;
  /** The option chosen now, which has a check mark, or -1. */
  selected?: number;
  onHighlight(index: number): void;
  onPick(index: number): void;
  /** A press outside the list and the field, the window losing the focus or resizing, or something around it scrolling. */
  onClose(): void;
}

const MARGIN = 8;
/** Between the field and the list. */
const GAP = 2;

/** The id of option `index` of the list `id`. */
export const optionId = (id: string, index: number) => `${id}-${index}`;

/**
 * The list a drop-down field opens (`Select`, `ComboBox`), drawn in the page rather than by the
 * platform, whose lists differ between WebView2, WebKit and GTK and ignore the theme. The field
 * keeps the focus and handles the keys, pointing screen readers at the highlighted option with
 * `aria-activedescendant`; the list only shows the options and takes the pointer's.
 *
 * It sits in the field's tree, often inside its `<label>`, and is placed in the window so that
 * a scrolling dialog doesn't clip it, from its size when it opens and again whenever its items
 * change (a combo box's, as the text is typed). A press on it keeps the focus in the field, and
 * its clicks don't reach the label, which would click the field again.
 */
export function ListPopup(props: Props) {
  // A new list for other items, placed from its own size rather than that of the last ones.
  const shown = props.items.map((item) => `${item.label}\t${item.detail ?? ""}`).join("\n");
  return <PlacedList key={shown} {...props} />;
}

function PlacedList({ id, anchor, labelledBy, items, highlight, selected = -1, onHighlight, onPick, onClose }: Props) {
  const ref = useRef<HTMLUListElement>(null);
  const [position, setPosition] = useState<CSSProperties | null>(null);
  // The option the pointer highlighted last: not scrolled into sight, which would move it from under the pointer.
  const pointed = useRef(-1);
  const onCloseEvent = useEffectEvent(onClose);

  useLayoutEffect(() => {
    const list = ref.current!;
    const field = anchor.current!.getBoundingClientRect();
    // As tall as the stylesheet allows, or the room there is.
    const natural = list.getBoundingClientRect();
    const tallest = parseFloat(getComputedStyle(list).maxHeight);
    const width = Math.min(Math.max(natural.width, field.width), window.innerWidth - 2 * MARGIN);
    const below = window.innerHeight - field.bottom - GAP - MARGIN;
    const above = field.top - GAP - MARGIN;
    const left = Math.max(MARGIN, Math.min(field.left, window.innerWidth - width - MARGIN));
    setPosition(
      natural.height <= below || below >= above
        ? { left, top: field.bottom + GAP, minWidth: width, maxHeight: Math.min(tallest, below) }
        : { left, bottom: window.innerHeight - field.top + GAP, minWidth: width, maxHeight: Math.min(tallest, above) },
    );
  }, [anchor]);

  useLayoutEffect(() => {
    const fromPointer = highlight === pointed.current;
    pointed.current = -1;
    if (position && highlight >= 0 && !fromPointer) document.getElementById(optionId(id, highlight))?.scrollIntoView({ block: "nearest" });
  }, [id, highlight, position]);

  useEffect(() => {
    const close = () => onCloseEvent();
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      const field = anchor.current;
      // The field opens and closes it itself.
      if (ref.current?.contains(target) || field?.contains(target)) return;
      // So does a press on the field's label, which clicks the field when released: the field
      // keeps the focus rather than closing the list as it loses it, then opening it again.
      if (field?.closest("label")?.contains(target)) e.preventDefault();
      else close();
    };
    const onScroll = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [anchor]);

  return (
    <ul
      ref={ref}
      id={id}
      className="list-popup"
      role="listbox"
      aria-labelledby={labelledBy}
      style={position ?? { visibility: "hidden" }}
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => e.preventDefault()}
    >
      {items.map((item, index) => (
        <li
          key={index}
          id={optionId(id, index)}
          role="option"
          aria-selected={index === highlight}
          className={index === highlight ? "list-popup-option highlighted" : "list-popup-option"}
          onMouseMove={() => {
            pointed.current = index;
            if (index !== highlight) onHighlight(index);
          }}
          onClick={() => onPick(index)}
        >
          <span className="list-popup-check" aria-hidden="true">
            {index === selected && <CheckIcon />}
          </span>
          <span className="list-popup-label" lang={item.lang}>
            {item.label}
          </span>
          {item.detail && <span className="list-popup-detail">{item.detail}</span>}
        </li>
      ))}
    </ul>
  );
}
