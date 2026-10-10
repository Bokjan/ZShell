import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorMessage, sessionsFile, type SessionsScan } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { forwardMapping } from "../lib/format";
import { address } from "../lib/sessions";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

interface Props {
  /** The exported sessions file. */
  path: string;
  onClose(): void;
  onImported(): void;
}

/** Imports sessions from a file exported by ZShell, skipping those that already exist. */
export function SessionImportDialog({ path, onClose, onImported }: Props) {
  const { t } = useTranslation();
  const [scan, setScan] = useState<SessionsScan | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dialog = useDialog(onClose);

  useEffect(() => {
    sessionsFile.scan(path).then(
      (found) => {
        setScan(found);
        setSelected(new Set(found.candidates.filter((c) => !c.existing).map((c) => c.id)));
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
      await sessionsFile.import(path, [...selected], scan!.digest);
      onImported();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  const candidates = scan?.candidates ?? null;
  const importable = candidates?.filter((c) => !c.existing) ?? [];
  const byId = new Map(candidates?.map((c) => [c.id, c]));
  // The jump hosts that the picked sessions bring with them (see `brings`), and which bring
  // each: they are imported too, so they are shown checked.
  const broughtBy = new Map<string, string[]>();
  for (const c of candidates ?? []) {
    if (!selected.has(c.id)) continue;
    for (const id of c.brings) broughtBy.set(id, [...(broughtBy.get(id) ?? []), c.name]);
  }
  const importing = new Set([...selected, ...broughtBy.keys()]);

  return (
    <Modal dialog={dialog}>
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
                    <input
                      type="checkbox"
                      checked={importing.has(c.id)}
                      disabled={!!c.existing || (broughtBy.has(c.id) && !selected.has(c.id))}
                      onChange={() => toggle(c.id)}
                    />
                    <span className="import-main">
                      <span className="import-alias">{c.name}</span>
                      <span className="import-address">{address(c.connection)}</span>
                    </span>
                  </label>
                  <div className="import-notes">
                    {c.folder.length > 0 && <span>{c.folder.join(" / ")}</span>}
                    {c.existing && <span>{t("importDialog.existing", { name: c.existing })}</span>}
                    {c.jumpHosts.length > 0 && <span>{t("importDialog.via", { names: c.jumpHosts.join(", ") })}</span>}
                    {!selected.has(c.id) && broughtBy.has(c.id) && (
                      <span>{t("sessionImport.broughtBy", { names: broughtBy.get(c.id)!.join(", ") })}</span>
                    )}
                    {c.brings.length > 0 && (
                      <span>{t("sessionImport.brings", { names: c.brings.map((id) => byId.get(id)?.name ?? id).join(", ") })}</span>
                    )}
                    {/* The first jump host's proxy is the one that connects this session. */}
                    {c.brings.map((id) => byId.get(id)).map(
                      (b) =>
                        b?.proxyCommand && (
                          <span key={b.id} className="warning">
                            {t("sessionImport.bringsProxyCommand", { name: b.name, command: b.proxyCommand })}
                          </span>
                        ),
                    )}
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
        {error && <ErrorText>{error}</ErrorText>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy || selected.size === 0}>
            {t("sessionImport.import", { count: importing.size })}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
