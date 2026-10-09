import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorMessage, sessionsFile, type SessionCandidate } from "../lib/api";
import { forwardMapping } from "../lib/format";
import { isComposing } from "../lib/platform";
import { address } from "../lib/sessions";

interface Props {
  /** The exported sessions file. */
  path: string;
  onClose(): void;
  onImported(): void;
}

/** Imports sessions from a file exported by ZShell, skipping those that already exist. */
export function SessionImportDialog({ path, onClose, onImported }: Props) {
  const { t } = useTranslation();
  const [candidates, setCandidates] = useState<SessionCandidate[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !isComposing(e) && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    sessionsFile.scan(path).then(
      (found) => {
        setCandidates(found);
        setSelected(new Set(found.filter((c) => !c.existing).map((c) => c.id)));
      },
      (e) => setError(errorMessage(e)),
    );
  }, [path]);

  const toggle = (id: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await sessionsFile.import(path, [...selected]);
      onImported();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  const importable = candidates?.filter((c) => !c.existing) ?? [];

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog import-dialog" onSubmit={submit}>
        <h2>{t("sessionImport.title")}</h2>
        <p className="hint import-path">{path}</p>

        {candidates && candidates.length > 0 && (
          <>
            <div className="import-select">
              <button type="button" className="link-button" onClick={() => setSelected(new Set(importable.map((c) => c.id)))}>
                {t("importDialog.selectAll")}
              </button>
              <button type="button" className="link-button" onClick={() => setSelected(new Set())}>
                {t("importDialog.selectNone")}
              </button>
            </div>
            <ul className="import-list">
              {candidates.map((c) => (
                <li key={c.id} className={c.existing ? "existing" : undefined}>
                  <label className="checkbox">
                    <input type="checkbox" checked={selected.has(c.id)} disabled={!!c.existing} onChange={() => toggle(c.id)} />
                    <span className="import-main">
                      <span className="import-alias">{c.name}</span>
                      <span className="import-address">{address(c)}</span>
                    </span>
                  </label>
                  <div className="import-notes">
                    {c.folder.length > 0 && <span>{c.folder.join(" / ")}</span>}
                    {c.existing && <span>{t("importDialog.existing", { name: c.existing })}</span>}
                    {c.jumpHosts.length > 0 && <span>{t("importDialog.via", { names: c.jumpHosts.join(", ") })}</span>}
                    {c.proxyCommand ? (
                      <span className="warning">{t("sessionImport.proxyCommand", { command: c.proxyCommand })}</span>
                    ) : (
                      c.proxy && <span>{t("sessionImport.proxy", { name: c.proxy })}</span>
                    )}
                    {c.autoForwards.map((rule, index) => (
                      <span key={index} className="warning">
                        {t("sessionImport.autoForward", { rule: forwardMapping(rule), kind: t(`forwards.kind.${rule.kind}`) })}
                      </span>
                    ))}
                  </div>
                </li>
              ))}
            </ul>
            <p className="hint">{t("sessionImport.hint")}</p>
          </>
        )}
        {candidates && candidates.length === 0 && <p className="hint">{t("sessionImport.empty")}</p>}
        {error && <p className="error">{error}</p>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy || selected.size === 0}>
            {t("sessionImport.import", { count: selected.size })}
          </button>
        </footer>
      </form>
    </div>
  );
}
