import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import type { CommandGroup, QuickCommand } from "../lib/api";
import { useDialog } from "../lib/dialogs";
import { groupName } from "../lib/quickCommands";
import { Modal } from "./Modal";

interface Props {
  /** Null creates a command. */
  command: QuickCommand | null;
  /** The group it is in, or goes in. */
  groupId: string;
  groups: CommandGroup[];
  onSave(groupId: string, command: QuickCommand): void;
  onClose(): void;
}

/** Creates or edits a quick command: its name, text, group, and whether Enter follows. */
export function CommandDialog({ command, groupId, groups, onSave, onClose }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState(command?.name ?? "");
  const [text, setText] = useState(command?.text ?? "");
  const [enter, setEnter] = useState(command?.enter ?? true);
  const [group, setGroup] = useState(groupId);

  const dialog = useDialog(onClose);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !text) return;
    onSave(group, { id: command?.id ?? "", name: name.trim(), text, enter });
    onClose();
  };

  return (
    <Modal dialog={dialog}>
      <form className="dialog command-dialog" onSubmit={submit}>
        <h2>{command ? t("quick.editTitle") : t("quick.newTitle")}</h2>
        <label>
          {t("quick.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} autoFocus placeholder={t("quick.namePlaceholder")} />
        </label>
        <label>
          {t("quick.text")}
          <textarea
            className="command-text"
            rows={5}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={t("quick.textPlaceholder")}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={enter} onChange={(e) => setEnter(e.target.checked)} />
          {t("quick.enter")}
        </label>
        <p className="hint">{t("quick.enterHint")}</p>
        <label>
          {t("quick.group")}
          <select value={group} onChange={(e) => setGroup(e.target.value)}>
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {groupName(g, t)}
              </option>
            ))}
          </select>
        </label>
        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!name.trim() || !text}>
            {t("common.save")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
