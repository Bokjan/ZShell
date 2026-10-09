import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import { errorMessage, sshConfig, type ImportCandidate } from "../lib/api";

interface Props {
  onClose(): void;
  onImported(): void;
}

/** Imports hosts from an OpenSSH client config file as session profiles. */
export function ImportDialog({ onClose, onImported }: Props) {
  const { t } = useTranslation();
  const [path, setPath] = useState("");
  const [candidates, setCandidates] = useState<ImportCandidate[] | null>(null);
  // The file the candidates come from, which the path field may no longer show.
  const [scanned, setScanned] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const load = async (file: string) => {
    if (!file.trim()) return;
    setBusy(true);
    try {
      const found = await sshConfig.scan(file.trim());
      setCandidates(found);
      setScanned(file.trim());
      // Hosts that already have a profile are not imported again.
      setSelected(new Set(found.filter((c) => !c.existing).map((c) => c.alias)));
      setError(null);
    } catch (e) {
      setCandidates(null);
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    sshConfig.defaultPath().then((file) => {
      if (!file) return;
      setPath(file);
      void load(file);
    }, console.error);
  }, []);

  const browse = async () => {
    const file = await open({ title: t("importDialog.chooseFile"), defaultPath: path || undefined });
    if (typeof file === "string") {
      setPath(file);
      void load(file);
    }
  };

  const toggle = (alias: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(alias)) next.add(alias);
      return next;
    });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await sshConfig.import(scanned, [...selected]);
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
        <h2>{t("importDialog.title")}</h2>

        <div className="field">
          <span>{t("importDialog.path")}</span>
          <div className="row">
            <input
              className="grow"
              aria-label={t("importDialog.path")}
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void load(path);
                }
              }}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
            />
            <button type="button" className="secondary" onClick={() => void load(path)} disabled={busy}>
              {t("importDialog.load")}
            </button>
            <button type="button" className="secondary" onClick={() => void browse()}>
              {t("importDialog.browse")}
            </button>
          </div>
        </div>

        {candidates && candidates.length > 0 && (
          <>
            <div className="import-select">
              <button
                type="button"
                className="link-button"
                onClick={() => setSelected(new Set(importable.map((c) => c.alias)))}
              >
                {t("importDialog.selectAll")}
              </button>
              <button type="button" className="link-button" onClick={() => setSelected(new Set())}>
                {t("importDialog.selectNone")}
              </button>
            </div>
            <ul className="import-list">
              {candidates.map((c) => (
                <li key={c.alias} className={c.existing ? "existing" : undefined}>
                  <label className="checkbox">
                    <input
                      type="checkbox"
                      checked={selected.has(c.alias)}
                      disabled={!!c.existing}
                      onChange={() => toggle(c.alias)}
                    />
                    <span className="import-main">
                      <span className="import-alias">{c.alias}</span>
                      <span className="import-address">
                        {c.username}@{c.host}
                        {c.port !== 22 && `:${c.port}`}
                      </span>
                    </span>
                  </label>
                  <div className="import-notes">
                    {c.existing && <span>{t("importDialog.existing", { name: c.existing })}</span>}
                    {c.jumpHosts.length > 0 && <span>{t("importDialog.via", { names: c.jumpHosts.join(", ") })}</span>}
                    {c.proxyCommand && <span>{t("importDialog.proxyCommand", { command: c.proxyCommand })}</span>}
                    {c.forwards.length > 0 && <span>{t("importDialog.forwards", { count: c.forwards.length })}</span>}
                    {c.skipped.length > 0 && (
                      <span className="warning">{t("importDialog.skipped", { options: c.skipped.join(", ") })}</span>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          </>
        )}
        {candidates && candidates.length === 0 && <p className="hint">{t("importDialog.empty")}</p>}
        {candidates && candidates.length > 0 && <p className="hint">{t("importDialog.hint")}</p>}

        {error && <p className="error">{error}</p>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy || selected.size === 0}>
            {t("importDialog.import", { count: selected.size })}
          </button>
        </footer>
      </form>
    </div>
  );
}
