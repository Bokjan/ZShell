import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorMessage, sessionsFile } from "../lib/api";
import { announce } from "../lib/announce";
import { useDialog } from "../lib/dialogs";
import { save as saveDialog } from "../lib/fileDialog";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

/**
 * Exports every session, with their folders and proxies, to a file chosen next; with the saved
 * passwords if asked, the whole file encrypted with a passphrase given here (twice, since it
 * can't be recovered).
 */
export function ExportDialog({ onClose }: { onClose(): void }) {
  const { t } = useTranslation();
  const dialog = useDialog(onClose);
  const [includePasswords, setIncludePasswords] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    if (includePasswords && !passphrase) return setError(t("exportDialog.passphraseEmpty"));
    if (includePasswords && passphrase !== confirm) return setError(t("exportDialog.passphrasesDontMatch"));
    const fileType = { name: t("sidebar.sessionsFileType"), extensions: ["json"] };
    const path = await saveDialog({ title: t("sidebar.exportSessions"), defaultPath: "zshell-sessions.json", filters: [fileType] });
    if (!path) return;
    setBusy(true);
    try {
      await sessionsFile.export(path, includePasswords ? passphrase : undefined);
      announce(t("exportDialog.done"));
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  return (
    <Modal dialog={dialog}>
      <form className="dialog" onSubmit={submit}>
        <h2>{t("exportDialog.title")}</h2>
        <p className="dialog-message">{t("exportDialog.message")}</p>
        <label className="checkbox">
          <input type="checkbox" checked={includePasswords} onChange={(e) => setIncludePasswords(e.target.checked)} />
          {t("exportDialog.includePasswords")}
        </label>
        {includePasswords && (
          <>
            <label>
              {t("exportDialog.passphrase")}
              <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoFocus />
            </label>
            <label>
              {t("exportDialog.confirmPassphrase")}
              <input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
            </label>
            <p className="hint">{t("exportDialog.passphraseHint")}</p>
          </>
        )}
        {error && <ErrorText>{error}</ErrorText>}
        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy}>
            {t("exportDialog.export")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
