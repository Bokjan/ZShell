import { useCallback, useRef, useState } from "react";

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
    try {
      await work();
    } finally {
      running.current = false;
      setBusy(false);
    }
  }, []);
  return { busy, submit };
}
