import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import type { QuickCommand, QuickCommands } from "../lib/api";
import { groupName, searchCommands } from "../lib/quickCommands";

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
  const matches = searchCommands(commands, query, firstGroup, t);
  const index = Math.min(selected, matches.length - 1);

  useEffect(() => {
    listRef.current?.querySelector(".palette-item.selected")?.scrollIntoView({ block: "nearest" });
  }, [index]);

  const run = (command: QuickCommand) => {
    onClose();
    onRun(command);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") setSelected(Math.min(index + 1, matches.length - 1));
    else if (e.key === "ArrowUp") setSelected(Math.max(index - 1, 0));
    else if (e.key === "Enter" && matches[index]) run(matches[index].command);
    else return;
    e.preventDefault();
  };

  return (
    <div className="dialog-backdrop palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`palette${targets ? " many" : ""}`} role="dialog" aria-label={t("quick.paletteTitle")}>
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
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        {targets && <div className="palette-targets">{t("quick.targets", { names: targets.join(", ") })}</div>}
        <div className="palette-list" ref={listRef}>
          {matches.length === 0 && (
            <p className="palette-empty">{query ? t("quick.noMatches") : t("quick.noCommands")}</p>
          )}
          {matches.map(({ group, command }, i) => (
            <div
              key={command.id}
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
    </div>
  );
}
