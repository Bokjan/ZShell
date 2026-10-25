import { cloneElement, isValidElement, useId, type ReactNode } from "react";

interface Props {
  /** Names the control, or the group of them. */
  title: ReactNode;
  /**
   * Says more about it, below it; screen readers read it with the control, or the group. A hint
   * about several fields (a row of them) goes after them as a `<p className="hint">` instead.
   */
  hint?: ReactNode;
  /**
   * A group of controls (radio buttons, checkboxes, a text field and its buttons): a `<fieldset>`
   * named by its `<legend>`. Otherwise a `<label>` around one control, which it names.
   */
  group?: boolean;
  className?: string;
  children: ReactNode;
}

/**
 * A field of a dialog with its title above it: the one way titles are written, so that they
 * all look the same (`.field-title`; `fields.test.ts` checks). One control is named by the
 * `<label>` around it; a group, which a `<label>` can't name (a `RadioGroup`, several
 * checkboxes), by its `<legend>`. See "无障碍" in docs/ARCHITECTURE.md.
 */
export function Field({ title, hint, group, className, children }: Props) {
  const hintId = useId();
  const describedBy = hint ? hintId : undefined;
  const hintText = hint && (
    <p className="hint" id={hintId}>
      {hint}
    </p>
  );
  if (group) {
    return (
      <>
        <fieldset className={className} aria-describedby={describedBy}>
          <legend className="field-title">{title}</legend>
          {children}
        </fieldset>
        {hintText}
      </>
    );
  }
  // The hint describes the control; every control a `Field` holds takes `aria-describedby`.
  const control =
    describedBy && isValidElement<{ "aria-describedby"?: string }>(children)
      ? cloneElement(children, { "aria-describedby": describedBy })
      : children;
  return (
    <>
      <label className={className ? `field ${className}` : "field"}>
        <span className="field-title">{title}</span>
        {control}
      </label>
      {hintText}
    </>
  );
}
