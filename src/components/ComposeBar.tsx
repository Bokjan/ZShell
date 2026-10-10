import { Fragment, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { announce } from "../lib/announce";
import type { Compose, ComposeScope, SendResult } from "../lib/compose";
import { IconButton } from "./IconButton";
import { ChevronIcon, CloseIcon } from "./icons";
import { paneTitle, tabTitle, type Tab } from "../lib/panes";
import { isComposing } from "../lib/platform";

interface Props {
  compose: Compose;
  tabs: Tab[];
  activeKey: number | null;
  followRemoteTitle: boolean;
  onChange(compose: Compose): void;
  /** Sends the text (as typed, with Enter) to the panes in scope. */
  onSend(text: string): SendResult;
  onClose(): void;
}

/** Sent texts, newest last; kept while the app runs, across closing and opening the bar. */
const history: string[] = [];
const HISTORY_LIMIT = 100;
/** The bar grows with the text up to this many lines, then scrolls. */
const MAX_ROWS = 6;
const STATUS_MS = 4000;

/**
 * Bar below the tab bar for typing a command and sending it to several tabs at once (Enter
 * sends, Shift+Enter adds a line, ↑↓ go through the history), and for syncing what is typed
 * in the terminal to them. Syncing turns the bar to the warning color.
 */
export function ComposeBar({ compose, tabs, activeKey, followRemoteTitle, onChange, onSend, onClose }: Props) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  // Position in `history` while going through it; `history.length` is the text being written.
  const historyIndex = useRef(history.length);
  const draft = useRef("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const scopeButtonRef = useRef<HTMLButtonElement>(null);
  const pickerId = useId();

  useEffect(() => inputRef.current?.focus(), []);

  useEffect(() => {
    if (status === null) return;
    const timer = setTimeout(() => setStatus(null), STATUS_MS);
    return () => clearTimeout(timer);
  }, [status]);

  // The scope picker closes on a click elsewhere or Escape, which gives the focus back to its
  // button if it was in the picker (it would otherwise be left on nothing).
  useEffect(() => {
    if (!picking) return;
    const onDown = (e: MouseEvent) => {
      if (!pickerRef.current?.contains(e.target as Node)) setPicking(false);
    };
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || isComposing(e)) return;
      if (pickerRef.current?.contains(document.activeElement)) scopeButtonRef.current?.focus();
      setPicking(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [picking]);

  // Shown for a few seconds, and said by screen readers (it appears beside the field).
  const show = (message: string) => {
    setStatus(message);
    announce(message);
  };

  const send = () => {
    if (!text.trim()) return;
    const { sent, skipped } = onSend(text);
    if (history[history.length - 1] !== text) history.push(text);
    if (history.length > HISTORY_LIMIT) history.shift();
    historyIndex.current = history.length;
    setText("");
    if (sent.length === 0) show(t("compose.noTargets"));
    else if (skipped > 0) show(t("compose.sentSkipped", { count: sent.length, skipped }));
    else show(t("compose.sent", { count: sent.length }));
  };


  const recall = (index: number) => {
    if (historyIndex.current === history.length) draft.current = text;
    historyIndex.current = index;
    setText(index === history.length ? draft.current : history[index]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (isComposing(e)) return;
    const input = e.currentTarget;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    } else if (e.key === "ArrowUp" && historyIndex.current > 0 && !input.value.slice(0, input.selectionStart).includes("\n")) {
      // Only from the first line, so ↑ still moves between lines of a multi-line text.
      e.preventDefault();
      recall(historyIndex.current - 1);
    } else if (
      e.key === "ArrowDown" &&
      historyIndex.current < history.length &&
      !input.value.slice(input.selectionEnd).includes("\n")
    ) {
      e.preventDefault();
      recall(historyIndex.current + 1);
    }
  };

  const setScope = (scope: ComposeScope) =>
    // Syncing to the current terminal alone means nothing.
    onChange({ ...compose, scope, sync: scope === "current" ? false : compose.sync });

  /** Selects the panes, or deselects them if they all are. */
  const toggleSelected = (keys: number[]) => {
    const all = keys.every((key) => compose.selected.includes(key));
    const selected = all
      ? compose.selected.filter((key) => !keys.includes(key))
      : [...compose.selected, ...keys.filter((key) => !compose.selected.includes(key))];
    onChange({ ...compose, scope: "selected", selected });
  };

  const panes = tabs.flatMap((tab) => tab.panes.map((pane) => ({ tab, pane })));
  const selectedCount = panes.filter(({ pane }) => compose.selected.includes(pane.key)).length;
  const tabPanes = tabs.find((tab) => tab.key === activeKey)?.panes.length ?? 0;
  const scopeName = {
    current: t("compose.scopeCurrent"),
    tab: t("compose.scopeTab"),
    all: t("compose.scopeAll"),
    selected: t("compose.scopeSelected"),
  };
  const scopeLabel = {
    current: t("compose.scopeCurrent"),
    tab: t("compose.scopeTabCount", { count: tabPanes }),
    all: t("compose.scopeAllCount", { count: panes.length }),
    selected: t("compose.scopeSelectedCount", { count: selectedCount }),
  }[compose.scope];
  const rows = Math.min(MAX_ROWS, text.split("\n").length);

  return (
    <div className={`compose-bar${compose.sync ? " syncing" : ""}`}>
      <div className="compose-scope" ref={pickerRef}>
        <button
          ref={scopeButtonRef}
          className={`compose-scope-button${compose.scope !== "current" ? " many" : ""}`}
          title={t("compose.scopeHint")}
          aria-haspopup="dialog"
          aria-expanded={picking}
          aria-controls={picking ? pickerId : undefined}
          onClick={() => setPicking(!picking)}
        >
          {scopeLabel}
          <ChevronIcon />
        </button>
        {picking && (
          <div className="compose-picker" id={pickerId} role="dialog" aria-label={t("compose.scopeHint")}>
            {(["current", "tab", "all", "selected"] as const).map((scope) => (
              <label key={scope} className="compose-picker-option">
                <input type="radio" name={pickerId} checked={compose.scope === scope} onChange={() => setScope(scope)} />
                {scopeName[scope]}
              </label>
            ))}
            <div className="compose-picker-tabs">
              {/* A split tab is listed with its panes below it; checking the tab checks them all. */}
              {tabs.map((tab) => {
                const keys = tab.panes.map((pane) => pane.key);
                const checked = keys.filter((key) => compose.selected.includes(key)).length;
                const single = tab.panes.length === 1;
                return (
                  <Fragment key={tab.key}>
                    <label className="compose-picker-tab">
                      <input
                        type="checkbox"
                        checked={checked === keys.length}
                        ref={(input) => {
                          if (input) input.indeterminate = checked > 0 && checked < keys.length;
                        }}
                        onChange={() => toggleSelected(keys)}
                      />
                      {single && <span className={`status-dot ${tab.panes[0].status}`} />}
                      <span className="compose-picker-title">{tabTitle(tab, followRemoteTitle)}</span>
                    </label>
                    {!single &&
                      tab.panes.map((pane) => (
                        <label key={pane.key} className="compose-picker-tab compose-picker-pane">
                          <input
                            type="checkbox"
                            checked={compose.selected.includes(pane.key)}
                            onChange={() => toggleSelected([pane.key])}
                          />
                          <span className={`status-dot ${pane.status}`} />
                          <span className="compose-picker-title">{paneTitle(pane, followRemoteTitle)}</span>
                        </label>
                      ))}
                  </Fragment>
                );
              })}
            </div>
          </div>
        )}
      </div>
      <div className="compose-field">
        <textarea
          ref={inputRef}
          className="compose-input"
          rows={rows}
          value={text}
          placeholder={t("compose.placeholder")}
          onChange={(e) => {
            setText(e.target.value);
            historyIndex.current = history.length;
          }}
          onKeyDown={onKeyDown}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        {/* Over the field's right end, so it doesn't push the controls around. */}
        {status && <span className="compose-status">{status}</span>}
      </div>
      <label
        className={`compose-sync${compose.scope === "current" ? " disabled" : ""}`}
        title={compose.scope === "current" ? t("compose.syncNeedsScope") : t("compose.syncHint")}
      >
        {/* Named by its text: WebKit would take the label's tooltip instead. */}
        <input
          type="checkbox"
          aria-label={t("compose.sync")}
          checked={compose.sync}
          disabled={compose.scope === "current"}
          onChange={(e) => onChange({ ...compose, sync: e.target.checked })}
        />
        {t("compose.sync")}
      </label>
      <button className="primary" disabled={!text.trim()} onClick={send}>
        {t("compose.send")}
      </button>
      <IconButton className="compose-close" label={t("compose.close")} onClick={onClose}>
        <CloseIcon />
      </IconButton>
    </div>
  );
}
