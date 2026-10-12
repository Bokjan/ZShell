import { useState } from "react";
import { useTranslation } from "react-i18next";

import { ProtectionDialog } from "../ProtectionDialog";
import { useVault } from "../VaultGate";
import { Section, Setting } from "./common";

/** How the sessions and passwords are protected (see `vault.rs`), changed in its own dialog. */
export function SecuritySection() {
  const { t } = useTranslation();
  const { status, setStatus } = useVault();
  const [changing, setChanging] = useState(false);
  const store = t(`vault.store.${status.store}`);
  const protection = status.protection ?? "unset";
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
