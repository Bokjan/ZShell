import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { sftp, type FileEntry, type SessionId } from "../../lib/api";
import { completionTarget, folderCandidates, tabCompletion } from "../../lib/pathCompletion";
import { isComposing } from "../../lib/platform";
import { FolderIcon } from "../icons";

interface Props {
  sessionId: SessionId | null;
  value: string;
  onChange(value: string): void;
  /** Shows the folder at `path`. */
  onGo(path: string): void;
  /** Whether hidden folders are offered without typing a dot. */
  showHidden: boolean;
}

/** How long typing pauses before the folder typed in is listed. */
const LIST_DELAY = 150;

/**
 * The file panel's path field, in its form (Enter goes to the path typed). While typing, the
 * folders in the folder typed so far that match the last part are offered below: Tab completes
 * as a shell does, the arrow keys pick one, Enter or a click goes to it, Escape closes the list.
 * Each folder is listed once while the field has the focus.
 */
export function PathField({ sessionId, value, onChange, onGo, showHidden }: Props) {
  const { t } = useTranslation();
  const listId = useId();
  const listRef = useRef<HTMLUListElement>(null);
  // Whether the value is the user's typing (rather than the folder shown), which is completed.
  const [typing, setTyping] = useState(false);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [listing, setListing] = useState<{ dir: string; entries: FileEntry[] } | null>(null);
  const cacheRef = useRef(new Map<string, FileEntry[]>());

  const { dir, prefix } = completionTarget(value);
  const candidates = typing && listing?.dir === dir ? folderCandidates(listing.entries, prefix, showHidden) : [];
  const shown = open && candidates.length > 0;

  useEffect(() => {
    if (!typing || sessionId == null) return;
    const cached = cacheRef.current.get(dir);
    if (cached) {
      setListing({ dir, entries: cached });
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      // A bare name is in the starting folder, as when it is entered.
      sftp.list(sessionId, dir || ".").then(
        (result) => {
          cacheRef.current.set(dir, result.entries);
          if (live) setListing({ dir, entries: result.entries });
        },
        // A folder that isn't there has nothing to offer.
        () => live && setListing({ dir, entries: [] }),
      );
    }, LIST_DELAY);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [typing, sessionId, dir]);

  useEffect(() => {
    if (active >= 0) listRef.current?.children[active]?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const type = (next: string) => {
    onChange(next);
    setTyping(true);
    setOpen(true);
    setActive(-1);
  };

  const go = (name: string) => {
    setTyping(false);
    setOpen(false);
    onGo(dir + name);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (isComposing(e)) return;
    const count = candidates.length;
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && count > 0) {
      const step = e.key === "ArrowDown" ? 1 : -1;
      setOpen(true);
      setActive((current) => (shown ? (current + step + count) % count : step > 0 ? 0 : count - 1));
    } else if (e.key === "Tab" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey) {
      // With nothing to complete, Tab moves on as usual.
      const completion = shown && active >= 0 ? `${candidates[active]}/` : tabCompletion(prefix, candidates);
      if (completion === null) return;
      type(dir + completion);
    } else if (e.key === "Enter") {
      if (!shown || active < 0) {
        // The form goes to the path typed.
        setTyping(false);
        setOpen(false);
        return;
      }
      go(candidates[active]);
    } else if (e.key === "Escape" && shown) {
      setOpen(false);
      setActive(-1);
      // Not closing the panel's other things too (the filter field, a selection).
      e.stopPropagation();
    } else return;
    e.preventDefault();
  };

  return (
    <>
      <input
        value={value}
        onChange={(e) => type(e.target.value)}
        onKeyDown={onKeyDown}
        onFocus={() => cacheRef.current.clear()}
        onBlur={() => {
          setTyping(false);
          setOpen(false);
          setActive(-1);
        }}
        title={t("sftp.pathHint")}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={shown}
        aria-controls={listId}
        aria-activedescendant={shown && active >= 0 ? `${listId}-${active}` : undefined}
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />
      <ul ref={listRef} id={listId} className="path-suggestions" role="listbox" aria-label={t("sftp.pathSuggestions")} hidden={!shown}>
        {shown &&
          candidates.map((name, i) => (
            <li
              key={name}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              className={`path-suggestion${i === active ? " selected" : ""}`}
              // The field keeps the focus (and the list stays) until the click goes there.
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => go(name)}
            >
              <FolderIcon />
              <span>{name}</span>
            </li>
          ))}
      </ul>
    </>
  );
}
