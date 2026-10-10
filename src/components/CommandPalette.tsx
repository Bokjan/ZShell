import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import type { QuickCommand, QuickCommands } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { clampIndex, navigateList } from "../lib/listNavigation";
import { groupName, searchCommands } from "../lib/quickCommands";
import { Modal } from "./Modal";

interface Props {
  commands: QuickCommands;
  /** The active tab's group, listed first. */
  firstGroup: string;
  /** As for `QuickCommandBar`: the tabs a command goes to when more than the active one. */
  targets: string[] | null;
  onRun(command: QuickCommand): void;
  onClose(): void;
}

/** Searches the quick commands of all groups; Enter (or a click) sends one. */
export function CommandPalette({ commands, firstGroup, targets, onRun, onClose }: Props) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();
  const optionId = (i: number) => `${listId}-${i}`;
  const matches = searchCommands(commands, query, firstGroup, t);
  const index = clampIndex(selected, matches.length);
  const dialog = useDialog(onClose);

  useEffect(() => {
    listRef.current?.querySelector(".palette-item.selected")?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const run = (command: QuickCommand) => {
    onClose();
    onRun(command);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    navigateList(e, matches.length, index, setSelected, (i) => run(matches[i].command));
  };

  return (
    <Modal dialog={dialog} label={t("quick.paletteTitle")} className="palette-backdrop">
      <div className={`palette${targets ? " many" : ""}`}>
        <input
          className="palette-input"
          value={query}
          placeholder={t("quick.palettePlaceholder")}
          autoFocus
          onChange={(e) => {
            setQuery(e.target.value);
            setSelected(0);
          }}
          onKeyDown={onKeyDown}
          role="combobox"
          aria-label={t("quick.paletteTitle")}
          aria-controls={listId}
          aria-expanded={matches.length > 0}
          aria-autocomplete="list"
          aria-activedescendant={index < 0 ? undefined : optionId(index)}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        {targets && <div className="palette-targets">{t("quick.targets", { names: targets.join(", ") })}</div>}
        {/* A list without items, which shows a message instead, has no role (see Sidebar). */}
        <div
          className="palette-list"
          ref={listRef}
          id={listId}
          role={matches.length > 0 ? "listbox" : undefined}
          aria-label={t("quick.paletteTitle")}
        >
          {matches.length === 0 && (
            <p className="palette-empty">{query ? t("quick.noMatches") : t("quick.noCommands")}</p>
          )}
          {matches.map(({ group, command }, i) => (
            <div
              key={command.id}
              id={optionId(i)}
              role="option"
              aria-selected={i === index}
              className={`palette-item${i === index ? " selected" : ""}`}
              onMouseEnter={() => setSelected(i)}
              onClick={() => run(command)}
            >
              <span className="palette-name">{command.name}</span>
              <span className="palette-group">{groupName(group, t)}</span>
              <span className="palette-text">{command.text}</span>
            </div>
          ))}
        </div>
      </div>
    </Modal>
  );
}
