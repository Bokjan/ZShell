import { useState } from "react";

import { SettingsDialog } from "../components/SettingsDialog";
import type { SectionId } from "../components/settings/common";

/**
 * The settings dialog, opened at a section if one is named. `dialog` is what to render.
 * `localAllowed` is false where local shells are blocked (Windows in S mode).
 */
export function useSettingsDialog(localAllowed: boolean) {
  // A new object each time they are asked for, so that a section asked for while they are
  // open is shown too.
  const [request, setRequest] = useState<{ section?: SectionId } | null>(null);

  const open = (section?: SectionId) => setRequest({ section });

  const dialog = request && <SettingsDialog localAllowed={localAllowed} request={request} onClose={() => setRequest(null)} />;

  return { open, isOpen: request !== null, dialog };
}
