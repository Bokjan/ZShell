import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import { errorMessage, sshConfig, type ImportCandidate } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { isComposing } from "../lib/platform";
import { CandidateList } from "./CandidateList";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

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

  const dialog = useDialog(onClose);
  // The latest scan: an earlier one finishing later (the default file, read as the dialog
  // opened, after a file chosen meanwhile) is dropped.
  const loads = useRef(0);

  const load = async (file: string) => {
    if (!file.trim()) return;
    const current = ++loads.current;
    setBusy(true);
    try {
      const found = await sshConfig.scan(file.trim());
      if (current !== loads.current) return;
      setCandidates(found);
      setScanned(file.trim());
      // Hosts that already have a profile are not imported again.
      setSelected(new Set(found.filter((c) => !c.existing).map((c) => c.alias)));
      setError(null);
    } catch (e) {
      if (current !== loads.current) return;
      setCandidates(null);
      setError(errorMessage(e));
    } finally {
      if (current === loads.current) setBusy(false);
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


  return (
    <Modal dialog={dialog}>
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
                if (e.key === "Enter" && !isComposing(e)) {
                  e.preventDefault();
                  // As the Load button: not while importing.
                  if (!busy) void load(path);
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
            <CandidateList
              candidates={candidates}
              keyOf={(c) => c.alias}
              isExisting={(c) => !!c.existing}
              selected={selected}
              onChange={setSelected}
              main={(c) => (
                <>
                  <span className="import-alias">{c.alias}</span>
                  <span className="import-address">
                    {c.username}@{c.host}
                    {c.port !== 22 && `:${c.port}`}
                  </span>
                </>
              )}
              notes={(c) => (
                <>
                  {c.existing && <span>{t("importDialog.existing", { name: c.existing })}</span>}
                  {c.jumpHosts.length > 0 && <span>{t("importDialog.via", { names: c.jumpHosts.join(", ") })}</span>}
                  {c.proxyCommand && <span>{t("importDialog.proxyCommand", { command: c.proxyCommand })}</span>}
                  {c.forwards.length > 0 && <span>{t("importDialog.forwards", { count: c.forwards.length })}</span>}
                  {c.skipped.length > 0 && <span className="warning">{t("importDialog.skipped", { options: c.skipped.join(", ") })}</span>}
                </>
              )}
            />
          </>
        )}
        {candidates && candidates.length === 0 && <p className="hint">{t("importDialog.empty")}</p>}
        {candidates && candidates.length > 0 && <p className="hint">{t("importDialog.hint")}</p>}

        {error && <ErrorText>{error}</ErrorText>}

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
    </Modal>
  );
}
