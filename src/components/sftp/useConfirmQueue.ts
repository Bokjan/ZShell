import { useRef, useState } from "react";

export interface Confirm {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  action(): void;
  /** A later question with the same key replaces this one, if it is still waiting. */
  key?: string;
}

/**
 * Questions shown one at a time: an edit conflict found by a background save waits behind a
 * delete or replace the user is answering, rather than taking its place.
 */
export function useConfirmQueue() {
  const [confirms, setConfirms] = useState<(Confirm & { id: number })[]>([]);
  const ids = useRef(0);
  const ask = (question: Confirm) => {
    const asked = { ...question, id: ++ids.current };
    setConfirms((queue) => {
      const at = question.key == null ? -1 : queue.findIndex((q, i) => i > 0 && q.key === question.key);
      return at < 0 ? [...queue, asked] : queue.map((q, i) => (i === at ? asked : q));
    });
  };
  const answered = () => setConfirms((queue) => queue.slice(1));
  return { confirm: confirms[0] ?? null, ask, answered };
}
