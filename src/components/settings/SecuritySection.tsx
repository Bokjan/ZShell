import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { appLock, errorCode, errorMessage, type AppLockSettings } from "../../ipc/api";
import { useAppLock } from "../../hooks/appLock";
import { ErrorText } from "../ErrorMessage";
import { ProtectionDialog } from "../ProtectionDialog";
import { useVault } from "../../hooks/vault";
import { attempt } from "../../lib/attempt";
import { useLocale } from "../../hooks/locale";
import { Section, Setting } from "./common";

/** The idle times offered, in minutes (as the backend accepts, `applock/mod.rs`); 0 never locks. */
const IDLE_MINUTES = [0, 1, 5, 15, 30, 60];

/**
 * How the sessions and passwords are protected (see `vault/`), changed in its own dialog,
 * and the app lock (see `applock/mod.rs`). Neither is in `settings.json`, so "Restore
 * defaults" changes neither.
 */
export function SecuritySection() {
  const { t } = useTranslation();
  const { status, setStatus } = useVault();
  const [changing, setChanging] = useState(false);
  const store = t(`vault.store.${status.store}`);
  // Unlocked, so known.
  const protection = status.protection!;
  const state = protection === "password" && status.remember ? "passwordRemembered" : protection;

  return (
    <Section id="security">
      <Setting>
        <p className="security-state">{t(`settings.protectionState.${state}`, { store })}</p>
        <div className="row">
          <button type="button" onClick={() => setChanging(true)}>
            {t("settings.protectionChange")}
          </button>
        </div>
        <p className="hint">{t("settings.securityHint")}</p>
      </Setting>
      <AppLockSetting />
      {changing && (
        <ProtectionDialog
          status={status}
          onDone={(next) => {
            setStatus(next);
            setChanging(false);
          }}
          onClose={() => setChanging(false)}
        />
      )}
    </Section>
  );
}

/**
 * Saved by the backend as soon as it changes, like the other settings, but through its own
 * command: turning it on asks the user to authenticate first, which shows that it works.
 */
function AppLockSetting() {
  const { t } = useTranslation();
  const { shortcutLabel } = useLocale();
  const { status, setStatus } = useAppLock();
  const vault = useVault().status;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The vault is unlocked while the settings show.
  const settings = status.settings!;
  // While the app lock is off, the system isn't asked what it offers until it shows here.
  const asked = status.method !== null || status.unavailable !== null;
  useEffect(() => {
    if (!asked) void appLock.status(true).then(setStatus, (e: unknown) => setError(errorMessage(e)));
  }, [asked, setStatus]);

  const save = async (next: AppLockSettings) => {
    setBusy(true);
    setError(null);
    return attempt(
      async () => {
        setStatus(await appLock.configure(next));
      },
      {
        onError: (e) => {
          if (errorCode(e) !== "appLock.cancelled") setError(errorMessage(e));
        },
        cleanup: () => {
          setBusy(false);
        },
      },
    );
  };

  if (!asked) return error && <ErrorText>{error}</ErrorText>;
  let hint: string;
  if (status.unavailable) hint = errorMessage(status.unavailable);
  else if (settings.enabled && vault.protection === "none") hint = t("settings.appLock.plainHint");
  else hint = t("settings.appLock.hint", { shortcut: shortcutLabel("lock") });

  return (
    <>
      <Setting>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={settings.enabled}
            disabled={busy || (!settings.enabled && status.method === null)}
            onChange={(e) => void save({ ...settings, enabled: e.target.checked })}
          />
          {t(`settings.appLock.enable.${status.method ?? "unavailable"}`)}
        </label>
        <p className="hint">{hint}</p>
        {error && <ErrorText>{error}</ErrorText>}
      </Setting>
      {settings.enabled && status.idle && (
        <Setting>
          <label>
            {t("settings.appLock.idle")}
            <select
              value={settings.idleMinutes}
              disabled={busy}
              onChange={(e) => void save({ ...settings, idleMinutes: Number(e.target.value) })}
            >
              {IDLE_MINUTES.map((minutes) => (
                <option key={minutes} value={minutes}>
                  {minutes === 0 ? t("settings.appLock.idleNever") : t("settings.appLock.idleMinutes", { count: minutes })}
                </option>
              ))}
            </select>
          </label>
        </Setting>
      )}
      {settings.enabled && status.systemLock && (
        <Setting>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={settings.onSystemLock}
              disabled={busy}
              onChange={(e) => void save({ ...settings, onSystemLock: e.target.checked })}
            />
            {t("settings.appLock.onSystemLock")}
          </label>
        </Setting>
      )}
    </>
  );
}
