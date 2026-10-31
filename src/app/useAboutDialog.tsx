import { useState } from "react";

import { AboutDialog } from "../components/AboutDialog";

/** The About dialog. `dialog` is what to render. */
export function useAboutDialog() {
  const [isOpen, setOpen] = useState(false);

  const open = () => setOpen(true);

  const dialog = isOpen && <AboutDialog onClose={() => setOpen(false)} />;

  return { open, isOpen, dialog };
}
