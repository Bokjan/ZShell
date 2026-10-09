import { useEffect, useRef, useState } from "react";

/**
 * A button that asks for a second click (delete, reset): the first click arms it and changes
 * its label, the second acts. Any click elsewhere disarms it, and so does the focus leaving
 * it, so that a much later click doesn't act at once. WebKit doesn't focus a button when it
 * is clicked, so a blur alone would not do.
 */
export function useConfirmButton() {
  const [armed, setArmed] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!armed) return;
    const disarm = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) setArmed(false);
    };
    window.addEventListener("pointerdown", disarm, true);
    return () => window.removeEventListener("pointerdown", disarm, true);
  }, [armed]);
  return { armed, setArmed, ref, onBlur: () => setArmed(false) };
}
