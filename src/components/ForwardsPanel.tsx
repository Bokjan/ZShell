import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  forwards,
  setProfileForwards,
  type ForwardRule,
  type ForwardState,
  type Profile,
  type SessionId,
} from "../lib/api";
import { hostPort } from "../lib/format";
import { ConfirmDialog } from "./ConfirmDialog";
import { ForwardDialog, explainRule } from "./ForwardDialog";

interface Props {
  sessionId: SessionId | null;
  connected: boolean;
  profile: Profile | undefined;
  /** Live rule states on this tab's connection, by rule id. */
  states: Record<string, ForwardState>;
  onProfileChanged(profile: Profile): void;
}

const isRunning = (state: ForwardState | undefined) => state?.type === "starting" || state?.type === "active";

const mapping = (rule: ForwardRule) => {
  const bind = hostPort(rule.bindHost, rule.bindPort);
  return rule.kind === "dynamic" ? bind : `${bind} → ${hostPort(rule.targetHost, rule.targetPort)}`;
};

export function ForwardsPanel({ sessionId, connected, profile, states, onProfileChanged }: Props) {
  const { t } = useTranslation();
  // undefined: dialog closed; null: adding a new rule.
  const [editing, setEditing] = useState<ForwardRule | null | undefined>(undefined);
  const [deleting, setDeleting] = useState<ForwardRule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rules = profile?.forwards ?? [];
  const live = connected && sessionId != null;

  const fail = (e: unknown) => setError(errorMessage(e));

  const start = (rule: ForwardRule) => sessionId != null && forwards.start(sessionId, rule).catch(fail);
  const stop = (rule: ForwardRule) => sessionId != null && forwards.stop(sessionId, rule.id).catch(fail);

  const save = async (rule: ForwardRule) => {
    if (!profile) return;
    const isNew = !rule.id;
    const list = isNew ? [...rules, rule] : rules.map((r) => (r.id === rule.id ? rule : r));
    const updated = await setProfileForwards(profile.id, list);
    onProfileChanged(updated);
    // New rules start right away; edited ones restart with their new definition if running.
    const saved = isNew ? updated.forwards[updated.forwards.length - 1] : updated.forwards.find((r) => r.id === rule.id);
    if (live && saved && (isNew || isRunning(states[saved.id]))) void start(saved);
  };

  const remove = async (rule: ForwardRule) => {
    setDeleting(null);
    if (!profile) return;
    try {
      // Also clears a failed state, so the tab's badge forgets the rule.
      if (live && states[rule.id]) await forwards.stop(sessionId, rule.id);
      onProfileChanged(await setProfileForwards(profile.id, rules.filter((r) => r.id !== rule.id)));
    } catch (e) {
      fail(e);
    }
  };

  const closeDialog = useCallback(() => setEditing(undefined), []);

  const status = (rule: ForwardRule, state: ForwardState | undefined) => {
    if (!state || state.type === "stopped") return t("forwards.status.stopped");
    switch (state.type) {
      case "starting":
        return t("forwards.status.starting");
      case "failed":
        return state.error.message;
      case "active": {
        const listening =
          rule.kind === "remote"
            ? t("forwards.status.listeningRemote", { address: state.bound })
            : t("forwards.status.listening", { address: state.bound });
        return state.connections > 0
          ? t("forwards.status.listeningWithConnections", { listening, count: state.connections })
          : listening;
      }
    }
  };

  if (!profile) {
    return (
      <div className="forwards-panel">
        <div className="forwards-empty">{t("forwards.profileMissing")}</div>
      </div>
    );
  }

  return (
    <div className="forwards-panel">
      <div className="forwards-toolbar">
        <span className="forwards-title">{t("forwards.title")}</span>
        <button onClick={() => setEditing(null)}>{t("forwards.add")}</button>
      </div>

      {error && (
        <div className="panel-error" onClick={() => setError(null)} title={t("sftp.dismissHint")}>
          {error}
        </div>
      )}
      {!live && rules.length > 0 && <div className="panel-note">{t("forwards.notConnected")}</div>}

      <ul className="forwards-list">
        {rules.map((rule) => {
          const state = states[rule.id];
          const running = isRunning(state);
          const lastError = state?.type === "active" ? state.lastError : null;
          return (
            <li key={rule.id} className={`forward-rule ${state?.type ?? "stopped"}`} title={explainRule(t, rule)}>
              <span className={`forward-kind ${rule.kind}`}>{t(`forwards.kind.${rule.kind}`)}</span>
              <div className="forward-main">
                <div className="forward-mapping">{mapping(rule)}</div>
                {rule.description && <div className="forward-description">{rule.description}</div>}
                <div className="forward-status">
                  <span className="status-dot" />
                  <span className="forward-status-text">{status(rule, state)}</span>
                  {rule.autoStart && <span className="forward-tag">{t("forwards.autoStartTag")}</span>}
                </div>
                {lastError && (
                  <div className="forward-last-error">{t("forwards.status.lastError", { error: lastError.message })}</div>
                )}
              </div>
              <span className="forward-actions">
                <button title={t("forwards.edit")} onClick={() => setEditing(rule)}>
                  ✎
                </button>
                <button title={t("forwards.delete")} onClick={() => setDeleting(rule)}>
                  🗑
                </button>
              </span>
              <label className="switch" title={running ? t("forwards.stop") : t("forwards.start")}>
                <input
                  type="checkbox"
                  checked={running}
                  disabled={!live}
                  onChange={() => void (running ? stop(rule) : start(rule))}
                />
                <span />
              </label>
            </li>
          );
        })}
      </ul>
      {rules.length === 0 && <div className="forwards-empty">{t("forwards.empty")}</div>}

      {editing !== undefined && <ForwardDialog rule={editing} onSave={save} onClose={closeDialog} />}
      {deleting && (
        <ConfirmDialog
          title={t("forwards.deleteTitle")}
          message={t("forwards.deleteMessage", { rule: mapping(deleting) })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void remove(deleting)}
          onCancel={() => setDeleting(null)}
        />
      )}
    </div>
  );
}
