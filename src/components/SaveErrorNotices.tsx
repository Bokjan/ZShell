import { useTranslation } from "react-i18next";

import { errorMessage } from "../ipc/api";
import { useSettings } from "../hooks/settings";
import { useQuickCommands } from "../hooks/stores";
import { quickCommandStore } from "../lib/quickCommands";
import { ConfirmDialog } from "./ConfirmDialog";

/** Tells of a change to the settings or the quick commands that couldn't be saved and was undone, maybe after its dialog closed. */
export function SaveErrorNotices() {
  const { t } = useTranslation();
  const { saveError: settingsError, dismissSaveError: dismissSettingsError } = useSettings();
  const { saveError: quickError } = useQuickCommands();

  return (
    <>
      {settingsError !== null && (
        <ConfirmDialog
          notice
          title={t("saveFailed.title")}
          message={t("saveFailed.settings", { error: errorMessage(settingsError) })}
          confirmLabel={t("common.ok")}
          onConfirm={dismissSettingsError}
          onCancel={dismissSettingsError}
        />
      )}
      {quickError !== null && (
        <ConfirmDialog
          notice
          title={t("saveFailed.title")}
          message={t("saveFailed.quickCommands", { error: errorMessage(quickError) })}
          confirmLabel={t("common.ok")}
          onConfirm={quickCommandStore.dismissSaveError}
          onCancel={quickCommandStore.dismissSaveError}
        />
      )}
    </>
  );
}
