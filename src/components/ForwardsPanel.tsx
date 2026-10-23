import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  FORWARD_CHANNELS_LISTED,
  forwards,
  profileForwardRules,
  profileForwards,
  type ForwardRule,
  type ForwardState,
  type Profile,
  type SessionId,
} from "../ipc/api";
import { directory } from "../lib/directory";
import { forwardMapping as mapping } from "../lib/format";
import { attempt } from "../lib/attempt";
import type { ForwardDetail, ForwardMeters } from "../lib/traffic";
import { ConfirmDialog } from "./ConfirmDialog";
import { ErrorBanner } from "./ErrorMessage";
import { ForwardDialog, explainRule } from "./ForwardDialog";
import { IconButton } from "./IconButton";
import { PencilIcon, TrashIcon } from "./icons";
import { TrafficLights, useTrafficText } from "./TrafficLights";

interface Props {
  sessionId: SessionId | null;
  connected: boolean;
  profile: Profile | undefined;
  /** A quick connection, which has no session to keep rules in. */
  quick: boolean;
  /** Live rule states on this tab's connection, by rule id. */
  states: Record<string, ForwardState>;
  /** What the rules that run carry. */
  traffic: ForwardMeters | undefined;
}

const isRunning = (state: ForwardState | undefined) => state?.type === "starting" || state?.type === "active";

/** Puts the session with its new rules in this window's directory; the other windows read theirs again. */
const profileSaved = (profile: Profile) => directory.profileSaved(profile);

export function ForwardsPanel({ sessionId, connected, profile, quick, states, traffic }: Props) {
  const { t } = useTranslation();
  const trafficText = useTrafficText();
  // undefined: dialog closed; null: adding a new rule.
  const [editing, setEditing] = useState<ForwardRule | null | undefined>(undefined);
  const [deleting, setDeleting] = useState<ForwardRule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rules = profile ? profileForwards(profile) : [];
  const live = connected && sessionId != null;

  const fail = (e: unknown) => setError(errorMessage(e));

  const start = (rule: ForwardRule) => sessionId != null && forwards.start(sessionId, rule).catch(fail);
  const stop = (rule: ForwardRule) => sessionId != null && forwards.stop(sessionId, rule.id).catch(fail);

  const save = async (rule: ForwardRule) => {
    if (!profile) return;
    const isNew = !rule.id;
    const { profile: updated, rule: saved } = await profileForwardRules.save(profile.id, rule);
    profileSaved(updated);
    // New rules start right away; edited ones restart with their new definition if running.
    if (live && (isNew || isRunning(states[saved.id]))) void start(saved);
  };

  const remove = async (rule: ForwardRule) => {
    setDeleting(null);
    if (!profile) return;
    return attempt(
      async () => {
        // Also clears a failed state, so the tab's badge forgets the rule.
        if (live && states[rule.id]) await forwards.stop(sessionId, rule.id);
        profileSaved(await profileForwardRules.delete(profile.id, rule.id));
      },
      {
        onError: fail,
      },
    );
  };

  const closeDialog = useCallback(() => setEditing(undefined), []);

  /** The tooltip of all the rules' lights: their channels on each connection they run on. */
  const allSummary = () => {
    const on = new Map<number, ForwardDetail>();
    for (const { detail } of traffic?.all() ?? []) {
      if (!detail) continue;
      const seen = on.get(detail.connection.id);
      on.set(
        detail.connection.id,
        seen
          ? { ...seen, channels: [...seen.channels, ...detail.channels], channelCount: seen.channelCount + detail.channelCount }
          : detail,
      );
    }
    const channels = [...on.values()].map(({ connection, channels, channelCount }) => {
      const listed = [...channels].sort((a, b) => a - b).slice(0, FORWARD_CHANNELS_LISTED);
      return trafficText.channels(connection, listed, channelCount);
    });
    return [t("traffic.allRules"), ...channels, ...trafficText.counts(traffic?.counts)];
  };

  /** The tooltip of a rule's lights. */
  const ruleSummary = (ruleId: string) => {
    const meter = traffic?.get(ruleId);
    const detail = meter?.detail;
    return [
      ...(detail ? [trafficText.channels(detail.connection, detail.channels, detail.channelCount)] : []),
      ...trafficText.counts(meter?.counts),
    ];
  };

  const status = (rule: ForwardRule, state: ForwardState | undefined) => {
    if (!state || state.type === "stopped") return t("forwards.status.stopped");
    switch (state.type) {
      case "starting":
        return t("forwards.status.starting");
      case "failed":
        return state.error.message;
      case "active": {
        return rule.kind === "remote"
          ? t("forwards.status.listeningRemote", { address: state.bound })
          : t("forwards.status.listening", { address: state.bound });
      }
    }
  };

  if (!profile) {
    return (
      <div className="forwards-panel">
        <div className="forwards-empty">{quick ? t("forwards.quickConnection") : t("forwards.profileMissing")}</div>
      </div>
    );
  }

  return (
    <div className="forwards-panel">
      <div className="forwards-toolbar">
        <span className="forwards-title">{t("forwards.title")}</span>
        {Object.values(states).some((state) => state.type === "active") && (
          <TrafficLights className="forwards-traffic" source={traffic} rates summary={allSummary} />
        )}
        <button onClick={() => setEditing(null)}>{t("forwards.add")}</button>
      </div>

      {error && <ErrorBanner onDismiss={() => setError(null)}>{error}</ErrorBanner>}
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
                  {/* Each part wraps as a whole, onto the next line, rather than squeezing the others. */}
                  <span className="forward-status-parts">
                    <span className="forward-status-text">{status(rule, state)}</span>
                    {state?.type === "active" && state.connections > 0 && (
                      <span className="forward-connections">{t("forwards.status.connections", { count: state.connections })}</span>
                    )}
                    {rule.autoStart && <span className="forward-tag">{t("forwards.autoStartTag")}</span>}
                  </span>
                </div>
                {state?.type === "active" && (
                  <div className="forward-traffic">
                    <TrafficLights source={traffic?.get(rule.id)} rates summary={() => ruleSummary(rule.id)} />
                  </div>
                )}
                {lastError && <div className="forward-last-error">{t("forwards.status.lastError", { error: lastError.message })}</div>}
              </div>
              <span className="forward-actions">
                <IconButton label={t("forwards.editRule", { rule: mapping(rule) })} onClick={() => setEditing(rule)}>
                  <PencilIcon />
                </IconButton>
                <IconButton label={t("forwards.deleteRule", { rule: mapping(rule) })} onClick={() => setDeleting(rule)}>
                  <TrashIcon />
                </IconButton>
              </span>
              <label className="switch" title={running ? t("forwards.stop") : t("forwards.start")}>
                <input
                  type="checkbox"
                  role="switch"
                  aria-label={t("forwards.runRule", { rule: mapping(rule) })}
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
