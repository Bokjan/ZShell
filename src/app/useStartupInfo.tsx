import { useEffect, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { configSetAside, localShellName, localUsername, type SetAsideFile } from "../ipc/api";
import { basename } from "../lib/format";

/**
 * What the app asks the backend once at startup: the local shell's name (or that local shells
 * are blocked), the user's name, and the configuration files set aside as unreadable, with the
 * dialog that tells about them.
 */
export function useStartupInfo() {
  const { t } = useTranslation();
  const [setAside, setSetAside] = useState<SetAsideFile[]>([]);
  // Title for local terminal tabs, e.g. "zsh".
  const [shellName, setShellName] = useState<string | null>(null);
  // False on Windows in S mode, which blocks local shells.
  const [localAllowed, setLocalAllowed] = useState(true);
  // For quick connections without a user name.
  const [username, setUsername] = useState("");

  // Returned only once, so a second call (StrictMode) must not clear the first's result.
  useEffect(() => {
    configSetAside()
      .then((files) => files.length > 0 && setSetAside(files))
      .catch(console.error);
  }, []);

  useEffect(() => {
    localShellName()
      .then((name) => (name === null ? setLocalAllowed(false) : setShellName(name)))
      .catch(console.error);
    localUsername().then(setUsername).catch(console.error);
  }, []);

  const setAsideDialog = setAside.length > 0 && (
    <ConfirmDialog
      title={t("setAside.title")}
      message={t("setAside.message")}
      confirmLabel={t("setAside.show")}
      cancelLabel={t("common.close")}
      onConfirm={() => {
        revealItemInDir(setAside[0].movedTo ?? setAside[0].path).catch(console.error);
        setSetAside([]);
      }}
      onCancel={() => setSetAside([])}
    >
      <ul className="set-aside-list">
        {setAside.map((file) => (
          <li key={file.path}>
            {file.movedTo
              ? t("setAside.moved", { name: basename(file.path), movedTo: basename(file.movedTo) })
              : t("setAside.notMoved", { name: basename(file.path) })}
            <span className="hint">{file.error}</span>
          </li>
        ))}
      </ul>
    </ConfirmDialog>
  );
  return { shellName, localAllowed, username, setAsideDialog };
}
