import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import type { Compose, ComposeScope } from "../lib/compose";
import { tabTitle, type Tab } from "./TabBar";

/** What happened to a send, for the bar's status line. */
export interface SendResult {
  sent: number;
  /** Tabs in scope that aren't connected. */
  skipped: number;
}

interface Props {
  compose: Compose;
  tabs: Tab[];
  followRemoteTitle: boolean;
  onChange(compose: Compose): void;
  /** Sends the text (as typed, with Enter) to the tabs in scope. */
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
export function ComposeBar({ compose, tabs, followRemoteTitle, onChange, onSend, onClose }: Props) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  // Position in `history` while going through it; `history.length` is the text being written.
  const historyIndex = useRef(history.length);
  const draft = useRef("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);

  useEffect(() => inputRef.current?.focus(), []);

  useEffect(() => {
    if (status === null) return;
    const timer = setTimeout(() => setStatus(null), STATUS_MS);
    return () => clearTimeout(timer);
  }, [status]);

  // The scope picker closes on a click elsewhere or Escape.
  useEffect(() => {
    if (!picking) return;
    const onDown = (e: MouseEvent) => {
      if (!pickerRef.current?.contains(e.target as Node)) setPicking(false);
    };
    const onKey = (e: globalThis.KeyboardEvent) => e.key === "Escape" && setPicking(false);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [picking]);

  const send = () => {
    if (!text.trim()) return;
    const { sent, skipped } = onSend(text);
    if (history[history.length - 1] !== text) history.push(text);
    if (history.length > HISTORY_LIMIT) history.shift();
    historyIndex.current = history.length;
    setText("");
    if (sent === 0) setStatus(t("compose.noTargets"));
    else if (skipped > 0) setStatus(t("compose.sentSkipped", { count: sent, skipped }));
    else setStatus(t("compose.sent", { count: sent }));
  };

  const recall = (index: number) => {
    if (historyIndex.current === history.length) draft.current = text;
    historyIndex.current = index;
    setText(index === history.length ? draft.current : history[index]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
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
    // Syncing to the current tab alone means nothing.
    onChange({ ...compose, scope, sync: scope === "current" ? false : compose.sync });

  const toggleSelected = (key: number) => {
    const selected = compose.selected.includes(key)
      ? compose.selected.filter((k) => k !== key)
      : [...compose.selected, key];
    onChange({ ...compose, scope: "selected", selected });
  };

  const selectedCount = tabs.filter((tab) => compose.selected.includes(tab.key)).length;
  const scopeLabel =
    compose.scope === "current"
      ? t("compose.scopeCurrent")
      : compose.scope === "all"
        ? t("compose.scopeAllCount", { count: tabs.length })
        : t("compose.scopeSelectedCount", { count: selectedCount });
  const rows = Math.min(MAX_ROWS, text.split("\n").length);

  return (
    <div className={`compose-bar${compose.sync ? " syncing" : ""}`}>
      <div className="compose-scope" ref={pickerRef}>
        <button
          className={`compose-scope-button${compose.scope !== "current" ? " many" : ""}`}
          title={t("compose.scopeHint")}
          onClick={() => setPicking(!picking)}
        >
          {scopeLabel} ▾
        </button>
        {picking && (
          <div className="compose-picker" role="dialog">
            {(["current", "all", "selected"] as const).map((scope) => (
              <label key={scope} className="compose-picker-option">
                <input type="radio" checked={compose.scope === scope} onChange={() => setScope(scope)} />
                {scope === "current"
                  ? t("compose.scopeCurrent")
                  : scope === "all"
                    ? t("compose.scopeAll")
                    : t("compose.scopeSelected")}
              </label>
            ))}
            <div className="compose-picker-tabs">
              {tabs.map((tab) => (
                <label key={tab.key} className="compose-picker-tab">
                  <input
                    type="checkbox"
                    checked={compose.selected.includes(tab.key)}
                    onChange={() => toggleSelected(tab.key)}
                  />
                  <span className={`status-dot ${tab.status}`} />
                  <span className="compose-picker-title">{tabTitle(tab, followRemoteTitle)}</span>
                </label>
              ))}
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
        <input
          type="checkbox"
          checked={compose.sync}
          disabled={compose.scope === "current"}
          onChange={(e) => onChange({ ...compose, sync: e.target.checked })}
        />
        {t("compose.sync")}
      </label>
      <button className="primary" disabled={!text.trim()} onClick={send}>
        {t("compose.send")}
      </button>
      <button className="compose-close" title={t("compose.close")} onClick={onClose}>
        ×
      </button>
    </div>
  );
}
