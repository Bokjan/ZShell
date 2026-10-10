import { useCallback, useMemo, useRef, useState } from "react";

import type { PasteTarget } from "../components/TerminalView";
import { CLOSED_COMPOSE, scopePanes, sendsToMany, syncTargets, type Compose, type SendResult } from "../lib/compose";
import { findPane, paneLabel, type Pane, type Tab } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { TabStore } from "../lib/tabs";

/** How long panes that received text from the compose bar flash. */
const FLASH_MS = 700;

interface Options {
  /** Whether panes are named after the title their program sets (the settings). */
  followRemoteTitle(): boolean;
  /** Moves the keyboard focus back to the active terminal (once the bar closes). */
  focusTerminal(): void;
}

/**
 * The compose bar and the panes it sends to: its scope, sending as if typed, syncing what is
 * typed in the focused pane (and pastes) to the others, and the panes flashing as they get it.
 */
export function useCompose(store: TabStore, sessions: SessionRegistry, tabs: Tab[], activeKey: number | null, options: Options) {
  const [compose, setCompose] = useState<Compose>(CLOSED_COMPOSE);
  const composeRef = useRef(compose);
  composeRef.current = compose;
  const [flashing, setFlashing] = useState<number[]>([]);
  const flashTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  // Closing the bar turns syncing off; the scope stays for the next time, shown on the bar.
  const toggle = useCallback(() => {
    const open = !composeRef.current.open;
    setCompose({ ...composeRef.current, open, sync: false });
    if (!open) optionsRef.current.focusTerminal();
  }, []);

  /** Names of panes, as listed for the compose bar and quick commands. */
  const labelsOf = (panes: Pane[]) =>
    panes.flatMap((pane) => {
      const found = findPane(store.get().tabs, pane.key);
      return found ? [paneLabel(found.tab, pane, optionsRef.current.followRemoteTitle())] : [];
    });

  /** Sends text as if typed to the panes in scope (see `scopePanes`); panes beyond the focused one flash. */
  const send = (data: string): SendResult => {
    const targets = scopePanes(composeRef.current, store.get().tabs, store.get().activeKey);
    const sent = targets.filter((pane) => sessions.get(pane.key)?.send(data));
    if (sendsToMany(composeRef.current) && sent.length > 0) {
      clearTimeout(flashTimer.current);
      setFlashing(sent.map((pane) => pane.key));
      flashTimer.current = setTimeout(() => setFlashing([]), FLASH_MS);
    }
    return { sent: labelsOf(sent), skipped: targets.length - sent.length };
  };

  // Syncing: what is typed in the focused terminal goes to the other connected panes in scope.
  const syncedWith = (source: number): Pane[] => syncTargets(composeRef.current, store.get().tabs, store.get().activeKey, source);
  const onInput = (source: number, data: string) => {
    for (const pane of syncedWith(source)) sessions.get(pane.key)?.send(data);
  };
  // Pastes go to each synced pane's terminal, which brackets them or not as its program wants.
  const pasteTargets = useRef(new Map<number, PasteTarget>());
  const registerPaste = (key: number, target: PasteTarget | null) => {
    if (target) pasteTargets.current.set(key, target);
    else pasteTargets.current.delete(key);
  };
  const syncedPasteTargets = (source: number) => syncedWith(source).flatMap((pane) => pasteTargets.current.get(pane.key) ?? []);

  const inScope = useMemo(() => (sendsToMany(compose) ? scopePanes(compose, tabs, activeKey) : []), [compose, tabs, activeKey]);
  // Who a quick command goes to, when that is more than the focused pane.
  const targets = sendsToMany(compose) ? labelsOf(inScope) : null;

  return { compose, setCompose, toggle, send, onInput, registerPaste, syncedPasteTargets, flashing, inScope, targets };
}
