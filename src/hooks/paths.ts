import { useEffect, useState } from "react";

import { homeDirectory } from "../ipc/api";

let home: Promise<string | null> | null = null;

/** The user's home folder (asked once), or null until it is known. */
export function useHomeDirectory() {
  const [dir, setDir] = useState<string | null>(null);
  useEffect(() => {
    home ??= homeDirectory().catch(() => null);
    void home.then(setDir);
  }, []);
  return dir;
}
