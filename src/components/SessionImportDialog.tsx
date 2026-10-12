import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorCode, errorMessage, sessionsFile, type SessionsScan } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { forwardMapping } from "../lib/format";
import { address } from "../lib/sessions";
import { CandidateList } from "./CandidateList";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

interface Props {
  /** The exported sessions file. */
  path: string;
  onClose(): void;
  onImported(): void;
}

/**
 * Imports sessions from a file exported by ZShell, skipping those that already exist, with
 * the passwords in the file if its passphrase is given.
 */
export function SessionImportDialog({ path, onClose, onImported }: Props) {
  const { t } = useTranslation();
  const [scan, setScan] = useState<SessionsScan | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // For the file's passwords; empty imports the sessions without them.
  const [passphrase, setPassphrase] = useState("");
  // Imported, but the passwords couldn't be saved: why.
  const [passwordsNotSaved, setPasswordsNotSaved] = useState<string | null>(null);

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


  const passphraseRef = useRef<HTMLInputElement>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (passwordsNotSaved) return onClose();
    setBusy(true);
    setError(null);
    try {
      const passwordError = await sessionsFile.import(path, [...selected], scan!.digest, passphrase || undefined);
      onImported();
      if (passwordError) {
        setPasswordsNotSaved(errorMessage(passwordError));
        setBusy(false);
      } else onClose();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
      if (errorCode(err) === "import.wrongPassphrase") requestAnimationFrame(() => passphraseRef.current?.select());
    }
  };

  const candidates = scan?.candidates ?? null;
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
            <CandidateList
              candidates={candidates}
              keyOf={(c) => c.id}
              isExisting={(c) => !!c.existing}
              selected={selected}
              onChange={setSelected}
              isChecked={(c) => importing.has(c.id)}
              isLocked={(c) => broughtBy.has(c.id) && !selected.has(c.id)}
              main={(c) => (
                <>
                  <span className="import-alias">{c.name}</span>
                  <span className="import-address">{address(c.connection)}</span>
                </>
              )}
              notes={(c) => (
                <>
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
                </>
              )}
            />
            <p className="hint">{t("sessionImport.hint")}</p>
          </>
        )}
        {candidates && candidates.length === 0 && <p className="hint">{t("sessionImport.empty")}</p>}
        {scan?.hasPasswords && candidates && candidates.length > 0 && !passwordsNotSaved && (
          <>
            <label>
              {t("sessionImport.passphrase")}
              <input ref={passphraseRef} type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
            </label>
            <p className="hint">{t("sessionImport.passphraseHint")}</p>
          </>
        )}
        {error && <ErrorText>{error}</ErrorText>}
        {passwordsNotSaved && <ErrorText>{t("sessionImport.passwordsNotSaved", { message: passwordsNotSaved })}</ErrorText>}

        <footer>
          <span className="grow" />
          {!passwordsNotSaved && (
            <button type="button" onClick={onClose}>
              {t("common.cancel")}
            </button>
          )}
          <button type="submit" className="primary" disabled={busy || (!passwordsNotSaved && selected.size === 0)}>
            {passwordsNotSaved ? t("common.close") : t("sessionImport.import", { count: importing.size })}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
