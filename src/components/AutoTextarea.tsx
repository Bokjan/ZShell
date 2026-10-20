import { useLayoutEffect, useRef, type ComponentProps } from "react";

type Props = Omit<ComponentProps<"textarea">, "ref"> & {
  /** The most lines it grows to before it scrolls; it starts at `rows`. */
  maxRows?: number;
};

/**
 * A multi-line field of a dialog that grows with its text, from `rows` lines up to `maxRows`,
 * then scrolls: in place of the handle each platform draws in its corner to drag it taller,
 * which in a dialog that scrolls is more often in the way than of use. Measured after each
 * change, so that the lines the text wraps onto count too.
 */
export function AutoTextarea({ maxRows = 12, ...props }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const { value } = props;
  useLayoutEffect(() => {
    const field = ref.current!;
    const style = getComputedStyle(field);
    const border = parseFloat(style.borderTopWidth) + parseFloat(style.borderBottomWidth);
    const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
    const line = parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.2;
    // Its height from `rows` first, so that the text's own shows in `scrollHeight` (padding
    // included, the border not; the field's box includes both).
    field.style.height = "auto";
    // Not laid out (a dialog in a hidden part of the window): as tall as `rows` until it changes.
    if (field.scrollHeight === 0) return;
    field.style.height = `${Math.min(field.scrollHeight, line * maxRows + padding) + border}px`;
  }, [value, maxRows]);
  return <textarea {...props} ref={ref} />;
}
