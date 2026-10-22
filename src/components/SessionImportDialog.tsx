import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorCode, errorMessage, sessionsFile, type SessionsScan } from "../ipc/api";
import { useDialog } from "../hooks/dialogs";
import { directory } from "../lib/directory";
import { forwardMapping } from "../lib/format";
import { address } from "../lib/sessions";
import { attempt } from "../lib/attempt";
import { CandidateList } from "./CandidateList";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";
import { PasswordInput } from "./PasswordInput";
import { useLocale } from "../hooks/locale";

interface Props {
  /** The exported sessions file. */
  path: string;
  onClose(): void;
}

/**
 * Imports sessions from a file exported by ZShell, skipping those that already exist. An
 * encrypted file (exported with its passwords) is opened with its passphrase first; its
 * passwords come along.
 */
export function SessionImportDialog({ path, onClose }: Props) {
  const { t } = useTranslation();
  const { formatList } = useLocale();
  const [scan, setScan] = useState<SessionsScan | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The file is encrypted: its passphrase opens it, and imports it.
  const [encrypted, setEncrypted] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const passphraseRef = useRef<HTMLInputElement>(null);

  const dialog = useDialog(onClose);

  const read = useCallback(
    async (passphrase?: string) => {
      return attempt(
        async () => {
          const found = await sessionsFile.scan(path, passphrase);
          setScan(found);
          setSelected(new Set(found.candidates.filter((c) => !c.existing).map((c) => c.id)));
        },
        {
          onError: (e) => {
            if (errorCode(e) === "import.passphraseRequired") return setEncrypted(true);
            setError(errorMessage(e));
            if (errorCode(e) === "import.wrongPassphrase") requestAnimationFrame(() => passphraseRef.current?.select());
          },
        },
      );
    },
    [path],
  );

  useEffect(() => void read(), [read]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    if (!scan) {
      // Opening the encrypted file.
      if (passphrase) await read(passphrase);
      return setBusy(false);
    }
    return attempt(
      async () => {
        await directory.changed(sessionsFile.import(path, [...selected], scan.digest, encrypted ? passphrase : undefined));
        onClose();
      },
      {
        onError: (err) => {
          setError(errorMessage(err));
          setBusy(false);
        },
      },
    );
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
                  {c.jumpHosts.length > 0 && <span>{t("importDialog.via", { names: formatList(c.jumpHosts) })}</span>}
                  {!selected.has(c.id) && broughtBy.has(c.id) && (
                    <span>{t("sessionImport.broughtBy", { names: formatList(broughtBy.get(c.id)!) })}</span>
                  )}
                  {c.brings.length > 0 && (
                    <span>{t("sessionImport.brings", { names: formatList(c.brings.map((id) => byId.get(id)?.name ?? id)) })}</span>
                  )}
                  {/* The first jump host's proxy is the one that connects this session. */}
                  {c.brings
                    .map((id) => byId.get(id))
                    .map(
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
                    c.proxy && <span>{t(c.existingProxy ? "sessionImport.existingProxy" : "sessionImport.proxy", { name: c.proxy })}</span>
                  )}
                  {c.autoForwards.map((rule, index) => (
                    <span key={index} className="warning">
                      {t("sessionImport.autoForward", { rule: forwardMapping(rule), kind: t(`forwards.kind.${rule.kind}`) })}
                    </span>
                  ))}
                  {c.connection.protocol === "ssh" && c.connection.forwardAgent && (
                    <span className="warning">{t("sessionImport.forwardAgent")}</span>
                  )}
                  {c.loginCommands.length > 0 && (
                    <span className="warning">{t("sessionImport.loginCommands", { commands: c.loginCommands.join("; ") })}</span>
                  )}
                  {c.connection.protocol === "ssh" && c.connection.env.length > 0 && (
                    <span>{t("sessionImport.env", { names: formatList(c.connection.env.map((v) => v.name)) })}</span>
                  )}
                </>
              )}
            />
            <p className="hint">{t("sessionImport.hint")}</p>
          </>
        )}
        {candidates && candidates.length === 0 && <p className="hint">{t("sessionImport.empty")}</p>}
        {encrypted && !scan && (
          <>
            <p className="dialog-message">{t("sessionImport.encrypted")}</p>
            <PasswordInput
              label={t("sessionImport.passphrase")}
              ref={passphraseRef}
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              disabled={busy}
              autoFocus
            />
          </>
        )}
        {error && <ErrorText>{error}</ErrorText>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          {scan ? (
            <button type="submit" className="primary" disabled={busy || selected.size === 0}>
              {t("sessionImport.import", { count: importing.size })}
            </button>
          ) : (
            encrypted && (
              <button type="submit" className="primary" disabled={busy || !passphrase}>
                {busy ? t("sessionImport.opening") : t("sessionImport.open")}
              </button>
            )
          )}
        </footer>
      </form>
    </Modal>
  );
}
