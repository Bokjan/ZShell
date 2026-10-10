import type { ComponentProps } from "react";

/**
 * A button that shows an icon (from `icons.tsx`) rather than words. `label` names it for
 * screen readers, which find no text in it, and is its tooltip. A `title` alone doesn't
 * name it: a button's text comes first, and `Tooltips` takes the `title` off while the
 * pointer is on the button.
 */
export function IconButton({ label, ...props }: { label: string } & Omit<ComponentProps<"button">, "title" | "aria-label">) {
  return <button {...props} aria-label={label} title={label} />;
}
