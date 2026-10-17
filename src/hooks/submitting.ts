import { useCallback, useRef, useState } from "react";

import { attempt } from "../lib/attempt";

/**
 * One request at a time for a dialog's buttons (save, delete): while one is running, others
 * are ignored, and `busy` disables the buttons. The ref, unlike the state, already holds for
 * a second click before React has rendered.
 */
export function useSubmitting() {
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  const submit = useCallback(async (work: () => Promise<void>) => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    return attempt(
      async () => {
        await work();
      },
      {
        cleanup: () => {
          running.current = false;
          setBusy(false);
        },
      },
    );
  }, []);
  return { busy, submit };
}
