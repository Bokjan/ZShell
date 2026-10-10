import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface Props<T> {
  candidates: T[];
  keyOf(candidate: T): string;
  /** Already here: not imported again, so unchecked and disabled. */
  isExisting(candidate: T): boolean;
  /** The keys picked. */
  selected: ReadonlySet<string>;
  onChange(selected: Set<string>): void;
  /** Whether a candidate's box is checked, when that isn't just whether it is picked. */
  isChecked?(candidate: T): boolean;
  /** Whether it can't be changed, besides existing ones. */
  isLocked?(candidate: T): boolean;
  /** Its name and address, beside the box. */
  main(candidate: T): ReactNode;
  /** What else to know about it, below. */
  notes(candidate: T): ReactNode;
}

/** What a file holds that can be imported, each with a box to pick it; with "Select all / none". */
export function CandidateList<T>({ candidates, keyOf, isExisting, selected, onChange, isChecked, isLocked, main, notes }: Props<T>) {
  const { t } = useTranslation();
  const importable = candidates.filter((c) => !isExisting(c));
  const toggle = (key: string) => {
    const next = new Set(selected);
    if (!next.delete(key)) next.add(key);
    onChange(next);
  };
  return (
    <>
      <div className="import-select">
        <button type="button" className="link-button" onClick={() => onChange(new Set(importable.map(keyOf)))}>
          {t("importDialog.selectAll")}
        </button>
        <button type="button" className="link-button" onClick={() => onChange(new Set())}>
          {t("importDialog.selectNone")}
        </button>
      </div>
      <ul className="import-list">
        {candidates.map((c) => {
          const key = keyOf(c);
          const existing = isExisting(c);
          return (
            <li key={key} className={existing ? "existing" : undefined}>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={isChecked ? isChecked(c) : selected.has(key)}
                  disabled={existing || !!isLocked?.(c)}
                  onChange={() => toggle(key)}
                />
                <span className="import-main">{main(c)}</span>
              </label>
              <div className="import-notes">{notes(c)}</div>
            </li>
          );
        })}
      </ul>
    </>
  );
}
