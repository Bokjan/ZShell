import { useState } from "react";

import { ImportDialog } from "../components/ImportDialog";

/** The dialog importing sessions from ssh_config. `dialog` is what to render. */
export function useImportDialog() {
  const [importing, setImporting] = useState(false);

  const open = () => setImporting(true);

  const dialog = importing && <ImportDialog onClose={() => setImporting(false)} />;

  return { open, dialog };
}
