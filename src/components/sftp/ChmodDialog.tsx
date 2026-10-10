import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import type { FileEntry } from "../../lib/api";
import { useDialog } from "../../lib/dialogs";
import { parseMode } from "../../lib/fileList";
import { formatMode } from "../../lib/format";
import { ErrorText } from "../ErrorMessage";
import { Modal } from "../Modal";

interface Props {
  entry: FileEntry;
  /** The new mode, valid. */
  onSubmit(mode: number): void;
  onClose(): void;
}

/** Changes a file's permissions, typed as an octal mode (`644`) with what it means shown below. */
export function ChmodDialog({ entry, onSubmit, onClose }: Props) {
  const { t } = useTranslation();
  const dialog = useDialog(onClose);
  const [value, setValue] = useState(((entry.permissions ?? 0o644) & 0o7777).toString(8).padStart(3, "0"));
  // Once OK was pressed with a value that isn't a mode, until it is changed.
  const [invalid, setInvalid] = useState(false);
  const mode = parseMode(value);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (mode === null) return setInvalid(true);
    onSubmit(mode);
  };

  return (
    <Modal dialog={dialog}>
      <form className="dialog" onSubmit={submit}>
        <h2>{t("sftp.chmodTitle")}</h2>
        <label>
          {t("sftp.chmodLabel", { name: entry.name })}
          <input
            autoFocus
            value={value}
            aria-invalid={invalid}
            onChange={(e) => {
              setValue(e.target.value);
              setInvalid(false);
            }}
          />
        </label>
        {invalid ? <ErrorText>{t("sftp.chmodInvalid")}</ErrorText> : <p className="hint">{mode !== null && formatMode(mode, entry.isDir, false)}</p>}
        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary">
            {t("common.ok")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
