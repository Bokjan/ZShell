import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { ISearchOptions, SearchAddon } from "@xterm/addon-search";

import { isComposing } from "../lib/platform";
import { IconButton } from "./IconButton";
import { ArrowIcon, CloseIcon } from "./icons";

interface Props {
  addon: SearchAddon;
  /** Highlight colors suited to the terminal's color scheme. */
  decorations: ISearchOptions["decorations"];
  /** Changes whenever the find shortcut is pressed again, to refocus the input. */
  focusKey: number;
  onClose(): void;
}

/** Matches beyond this many are not highlighted or counted. */
export const HIGHLIGHT_LIMIT = 1000;

type Flag = "caseSensitive" | "wholeWord" | "regex";

const FLAGS: { flag: Flag; label: string; title: "search.caseSensitive" | "search.wholeWord" | "search.regex" }[] = [
  { flag: "caseSensitive", label: "Aa", title: "search.caseSensitive" },
  { flag: "wholeWord", label: "ab", title: "search.wholeWord" },
  { flag: "regex", label: ".*", title: "search.regex" },
];

/** Find bar floating over the terminal, backed by xterm's search addon. */
export function SearchBar({ addon, decorations, focusKey, onClose }: Props) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [flags, setFlags] = useState<Record<Flag, boolean>>({ caseSensitive: false, wholeWord: false, regex: false });
  const [result, setResult] = useState<{ index: number; count: number } | null>(null);
  const [invalid, setInvalid] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusKey]);

  useEffect(() => {
    const subscription = addon.onDidChangeResults(({ resultIndex, resultCount }) =>
      setResult({ index: resultIndex, count: resultCount }),
    );
    return () => {
      subscription.dispose();
      addon.clearDecorations();
    };
  }, [addon]);

  const find = (forward: boolean, incremental = false) => {
    if (!query) return;
    const options: ISearchOptions = { ...flags, incremental, decorations };
    try {
      if (forward) addon.findNext(query, options);
      else addon.findPrevious(query, options);
      setInvalid(false);
    } catch {
      // An incomplete regular expression while typing.
      addon.clearDecorations();
      setInvalid(true);
    }
  };

  // Search as you type, extending the current match where possible.
  useEffect(() => {
    // Also resets the addon's cached term: it compares options only after storing the new
    // ones, so toggling a flag alone would otherwise never re-highlight (addon-search 0.16).
    addon.clearDecorations();
    if (query) find(true, true);
    else {
      setResult(null);
      setInvalid(false);
    }
  }, [query, flags]);

  const onInputKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !isComposing(e)) {
      e.preventDefault();
      find(!e.shiftKey);
    }
  };

  // Escape closes the bar from any of its controls, not just the input.
  const onBarKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && !isComposing(e)) {
      e.preventDefault();
      onClose();
    }
  };

  let status = "";
  if (invalid) status = t("search.invalidRegex");
  else if (query && result) {
    if (result.count === 0) status = t("search.noResults");
    else if (result.count < 0) status = t("search.tooManyResults", { limit: HIGHLIGHT_LIMIT });
    else if (result.index < 0) status = t("search.results", { count: result.count });
    else status = t("search.position", { current: result.index + 1, total: result.count });
  }

  return (
    <div
      className="search-bar"
      onKeyDown={onBarKeyDown}
      // Keep focus in the input when clicking the buttons (WebKit would otherwise drop it
      // to the page), so typing, Enter and Escape keep working.
      onMouseDown={(e) => e.target !== inputRef.current && e.preventDefault()}
    >
      <input
        ref={inputRef}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={onInputKeyDown}
        placeholder={t("search.placeholder")}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
      />
      {FLAGS.map(({ flag, label, title }) => (
        <IconButton
          key={flag}
          className={`search-flag${flags[flag] ? " on" : ""}`}
          aria-pressed={flags[flag]}
          label={t(title)}
          onClick={() => setFlags((f) => ({ ...f, [flag]: !f[flag] }))}
        >
          {label}
        </IconButton>
      ))}
      {/* Read out as it changes: the focus stays in the field while the matches are counted and visited. */}
      <span className={`search-status${invalid || result?.count === 0 ? " empty" : ""}`} role="status">
        {status}
      </span>
      <IconButton className="icon-button" label={t("search.previous")} disabled={!query} onClick={() => find(false)}>
        <ArrowIcon direction="up" />
      </IconButton>
      <IconButton className="icon-button" label={t("search.next")} disabled={!query} onClick={() => find(true)}>
        <ArrowIcon direction="down" />
      </IconButton>
      <IconButton className="icon-button" label={t("search.close")} onClick={onClose}>
        <CloseIcon />
      </IconButton>
    </div>
  );
}
