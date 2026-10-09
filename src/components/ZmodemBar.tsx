import { useTranslation } from "react-i18next";

import type { ZmodemPhase } from "../lib/api";

interface Props {
  phase: Exclude<ZmodemPhase, "idle">;
  /** Files are being dragged over the terminal (while `rz` waits). */
  dragOver: boolean;
  onChooseFiles(): void;
  onSaveToDownloads(): void;
  onChooseFolder(): void;
  onCancel(): void;
}

/**
 * Bar over the terminal during a ZMODEM transfer: while `sz` waits for where to save (the
 * download folder, another folder, or cancel), while `rz` waits for files (choose, drop or
 * cancel), and while a transfer runs (cancel). Progress itself is written in the terminal.
 */
export function ZmodemBar({ phase, dragOver, onChooseFiles, onSaveToDownloads, onChooseFolder, onCancel }: Props) {
  const { t } = useTranslation();
  return (
    // Buttons don't take focus from the terminal, where Ctrl+C also cancels.
    <div className={`zmodem-bar${dragOver ? " drag-over" : ""}`} onMouseDown={(e) => e.preventDefault()}>
      <span className="zmodem-text">
        {phase === "chooseFiles"
          ? dragOver
            ? t("zmodem.dropToSend")
            : t("zmodem.waitingForFiles")
          : phase === "chooseDestination"
            ? t("zmodem.receiving")
            : t("zmodem.transferring")}
      </span>
      {phase === "chooseDestination" && (
        <>
          <button className="primary" onClick={onSaveToDownloads}>
            {t("zmodem.saveToDownloads")}
          </button>
          <button onClick={onChooseFolder}>{t("zmodem.chooseFolder")}</button>
        </>
      )}
      {phase === "chooseFiles" && (
        <button className="primary" onClick={onChooseFiles}>
          {t("zmodem.chooseFiles")}
        </button>
      )}
      <button onClick={onCancel}>{t("common.cancel")}</button>
    </div>
  );
}
