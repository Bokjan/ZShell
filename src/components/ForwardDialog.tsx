import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";

import { errorMessage, type ForwardKind, type ForwardRule } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { hostPort, parsePort } from "../lib/format";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

interface Props {
  /** null creates a new rule. */
  rule: ForwardRule | null;
  /** Rejects with a backend error to keep the dialog open. */
  onSave(rule: ForwardRule): Promise<void>;
  onClose(): void;
}

/** Loopback by default, as OpenSSH does; the server resolves "localhost" itself. */
const defaultBindHost = (kind: ForwardKind) => (kind === "remote" ? "localhost" : "127.0.0.1");

const isLoopback = (host: string) => host === "" || host === "localhost" || host === "::1" || /^127\./.test(host);

/** One sentence describing what the rule does, e.g. for tooltips. */
export function explainRule(t: TFunction, rule: Pick<ForwardRule, "kind" | "bindHost" | "bindPort" | "targetHost" | "targetPort">) {
  const bind = hostPort(rule.bindHost || defaultBindHost(rule.kind), rule.bindPort);
  const target = hostPort(rule.targetHost, rule.targetPort);
  return t(`forwards.explain.${rule.kind}`, { bind, target });
}

export function ForwardDialog({ rule, onSave, onClose }: Props) {
  const { t } = useTranslation();
  const [kind, setKind] = useState<ForwardKind>(rule?.kind ?? "local");
  const [bindHost, setBindHost] = useState(rule?.bindHost ?? defaultBindHost("local"));
  const [bindPort, setBindPort] = useState(rule ? String(rule.bindPort) : "");
  const [targetHost, setTargetHost] = useState(rule?.targetHost ?? "localhost");
  const [targetPort, setTargetPort] = useState(rule?.targetPort ? String(rule.targetPort) : "");
  const [description, setDescription] = useState(rule?.description ?? "");
  const [autoStart, setAutoStart] = useState(rule?.autoStart ?? false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const dialog = useDialog(onClose);

  const changeKind = (next: ForwardKind) => {
    // Follow the default bind address unless the user changed it.
    if (bindHost.trim() === defaultBindHost(kind)) setBindHost(defaultBindHost(next));
    setKind(next);
  };

  const dynamic = kind === "dynamic";
  const bindPortNumber = parsePort(bindPort, { allowZero: true });
  const targetPortNumber = parsePort(targetPort);
  const host = bindHost.trim();

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (bindPortNumber === null) {
      setError(t("forwards.dialog.invalidBindPort"));
      return;
    }
    if (!dynamic && (!targetHost.trim() || targetPortNumber === null)) {
      setError(t("forwards.dialog.invalidTarget"));
      return;
    }
    setSaving(true);
    try {
      await onSave({
        id: rule?.id ?? "",
        kind,
        bindHost: host,
        bindPort: bindPortNumber,
        targetHost: dynamic ? "" : targetHost.trim(),
        targetPort: dynamic || targetPortNumber === null ? 0 : targetPortNumber,
        description: description.trim(),
        autoStart,
      });
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
    }
  };

  const listenLabel = { local: "forwards.dialog.listenLocal", remote: "forwards.dialog.listenRemote", dynamic: "forwards.dialog.listenSocks" } as const;
  const preview = explainRule(t, {
    kind,
    bindHost: host,
    bindPort: bindPortNumber ?? 0,
    targetHost: targetHost.trim() || "…",
    targetPort: targetPortNumber ?? 0,
  });

  return (
    <Modal dialog={dialog}>
      <form className="dialog forward-dialog" onSubmit={submit}>
        <h2>{rule ? t("forwards.dialog.titleEdit") : t("forwards.dialog.titleNew")}</h2>

        <label>
          {t("forwards.dialog.type")}
          <select value={kind} onChange={(e) => changeKind(e.target.value as ForwardKind)}>
            <option value="local">{t("forwards.kindName.local")}</option>
            <option value="remote">{t("forwards.kindName.remote")}</option>
            <option value="dynamic">{t("forwards.kindName.dynamic")}</option>
          </select>
        </label>

        <fieldset>
          <legend>{t(listenLabel[kind])}</legend>
          <div className="row">
            <label className="grow">
              {t("forwards.dialog.address")}
              <input
                value={bindHost}
                onChange={(e) => setBindHost(e.target.value)}
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
              />
            </label>
            <label className="port">
              {t("forwards.dialog.port")}
              <input
                value={bindPort}
                onChange={(e) => setBindPort(e.target.value)}
                inputMode="numeric"
                required
                autoFocus
                title={t("forwards.dialog.portZeroHint")}
              />
            </label>
          </div>
          {!isLoopback(host) && (
            <p className="hint warning">
              {kind === "remote" ? t("forwards.dialog.exposedRemote") : t("forwards.dialog.exposedLocal")}
            </p>
          )}
        </fieldset>

        {!dynamic && (
          <fieldset>
            <legend>{kind === "local" ? t("forwards.dialog.targetFromServer") : t("forwards.dialog.targetFromLocal")}</legend>
            <div className="row">
              <label className="grow">
                {t("forwards.dialog.address")}
                <input
                  value={targetHost}
                  onChange={(e) => setTargetHost(e.target.value)}
                  required
                  autoCapitalize="off"
                  autoCorrect="off"
                  spellCheck={false}
                />
              </label>
              <label className="port">
                {t("forwards.dialog.port")}
                <input value={targetPort} onChange={(e) => setTargetPort(e.target.value)} inputMode="numeric" required />
              </label>
            </div>
          </fieldset>
        )}

        <p className="hint">{preview}</p>

        <label>
          {t("forwards.dialog.description")}
          <input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t("forwards.dialog.descriptionPlaceholder")}
          />
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={autoStart} onChange={(e) => setAutoStart(e.target.checked)} />
          {t("forwards.dialog.autoStart")}
        </label>

        {error && <ErrorText>{error}</ErrorText>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={saving}>
            {t("common.save")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
