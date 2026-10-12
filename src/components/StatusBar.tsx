import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Profile } from "../lib/api";
import { formatAgo, formatSize } from "../lib/format";
import type { Pane } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { address, serialSummary } from "../lib/sessions";
import { useMediaQuery } from "../lib/settings";
import type { TrafficMeter } from "../lib/traffic";
import { ActivityIcon } from "./icons";

interface Props {
  /** The focused pane of the active tab. */
  pane: Pane;
  /** The pane's saved session; undefined for local terminals, quick connections and deleted sessions. */
  profile: Profile | undefined;
  sessions: SessionRegistry;
}

/**
 * How long a light stays on after data was sent or received: steady while data keeps coming.
 * Longer when the system asks for less motion, so it blinks less.
 */
const LIGHT_MS = 150;
const LIGHT_REDUCED_MS = 400;

/**
 * The bar under the terminals, about the focused pane: its connection, the lights for data
 * sent and received, the terminal type (or a serial port's line settings), the terminal's
 * size and the cursor's position. Not a live region: the cursor moves too often to announce.
 */
export function StatusBar({ pane, profile, sessions }: Props) {
  const { t } = useTranslation();
  const screen = useScreen(sessions, pane.key);
  const connection = profile?.connection;
  const serial = connection?.protocol === "serial" ? connection : null;
  return (
    <footer className="status-bar" role="group" aria-label={t("statusBar.label")}>
      <Connection pane={pane} profile={profile} />
      <Activity meter={sessions.get(pane.key)?.traffic} />
      {serial ? (
        <Item
          text={serialSummary(serial)}
          label={t("statusBar.serialLineLabel", { line: serialSummary(serial) })}
          title={t("statusBar.serialLine")}
        />
      ) : (
        pane.termType && (
          <Item
            text={pane.termType}
            label={t("statusBar.termTypeLabel", { type: pane.termType })}
            title={t(pane.protocol === "local" ? "statusBar.localTermType" : "statusBar.termType")}
          />
        )
      )}
      {screen && (
        <>
          <Item className="status-size" text={t("statusBar.size", screen)} label={t("statusBar.sizeLabel", screen)} />
          <Item className="status-cursor" text={t("statusBar.cursor", screen)} label={t("statusBar.cursorLabel", screen)} />
        </>
      )}
    </footer>
  );
}

/** Shown as `text`, read as `label` (a whole sentence rather than "98x37"), with `title` as its tooltip. */
function Item({ text, label, title, className }: { text: string; label: string; title?: string; className?: string }) {
  return (
    <span className={className} title={title ?? label}>
      <span aria-hidden="true">{text}</span>
      <span className="visually-hidden">{label}</span>
    </span>
  );
}

/** Where the pane connects, and how its connection is doing unless it is up. */
function Connection({ pane, profile }: { pane: Pane; profile: Profile | undefined }) {
  const { t } = useTranslation();
  const seconds = useCountdown(pane.status === "closed" ? pane.retryAt : null);
  const { target } = pane;
  let where: string;
  if (target.kind === "local") where = t("statusBar.local");
  else if (target.kind === "quick") where = address(target);
  else if (!profile) where = pane.title;
  else if (profile.connection.protocol === "serial") where = profile.connection.device;
  else where = address(profile.connection);
  let state: string | null = null;
  if (pane.status === "connecting") state = t("tabs.status.connecting");
  else if (seconds !== null) state = t("statusBar.reconnectIn", { count: seconds });
  else if (pane.status === "closed") state = t(pane.protocol === "local" ? "tabs.status.exited" : "tabs.status.closed");
  return (
    <span className="status-connection">
      <span className={`status-dot ${pane.status}`} />
      <span className="status-address">{where}</span>
      {state && <span className="status-state">{state}</span>}
    </span>
  );
}

/**
 * The send and receive lights, with what the connection has carried in the tooltip. Drawn
 * again each second, for the tooltip's "last received" (taken when the pointer arrives).
 */
function Activity({ meter }: { meter: TrafficMeter | undefined }) {
  const { t } = useTranslation();
  const reduced = useMediaQuery("(prefers-reduced-motion: reduce)");
  const { send, receive } = useLights(meter, reduced ? LIGHT_REDUCED_MS : LIGHT_MS);
  const now = useTick(1000);
  const sent = formatSize(meter?.sent ?? 0);
  const received = formatSize(meter?.received ?? 0);
  const last = meter?.lastReceived;
  const lines = [
    t("statusBar.traffic", { sent, received }),
    last == null ? t("statusBar.nothingReceived") : t("statusBar.lastReceived", { time: formatAgo(now - last) }),
  ];
  return (
    <span className="status-activity" title={[...lines, t("statusBar.trafficScope")].join("\n")}>
      <span className={send ? "send on" : "send"}>
        <ActivityIcon direction="up" />
      </span>
      <span className={receive ? "receive on" : "receive"}>
        <ActivityIcon direction="down" />
      </span>
      <span className="visually-hidden">{lines.join(" ")}</span>
    </span>
  );
}

/** Whether data was sent and received within the last `hold` milliseconds, at most once a frame. */
function useLights(meter: TrafficMeter | undefined, hold: number) {
  const [lights, setLights] = useState({ send: false, receive: false });
  useEffect(() => {
    let frame = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const update = () => {
      frame = 0;
      clearTimeout(timer);
      const now = Date.now();
      const until = (at: number | null | undefined) => (at == null ? 0 : at + hold - now);
      const send = until(meter?.lastSent);
      const receive = until(meter?.lastReceived);
      setLights((lights) =>
        lights.send === send > 0 && lights.receive === receive > 0 ? lights : { send: send > 0, receive: receive > 0 },
      );
      // Off again once the newer of the two has had its time.
      const next = Math.min(...[send, receive].filter((left) => left > 0));
      if (Number.isFinite(next)) timer = setTimeout(update, next);
    };
    update();
    const unsubscribe = meter?.subscribe(() => {
      if (!frame) frame = requestAnimationFrame(update);
    });
    return () => {
      unsubscribe?.();
      cancelAnimationFrame(frame);
      clearTimeout(timer);
    };
  }, [meter, hold]);
  return lights;
}

/** The pane's terminal size and cursor, at most once a frame; null until its terminal is shown. */
function useScreen(sessions: SessionRegistry, key: number) {
  const [state, setState] = useState<{ cols: number; rows: number; row: number; column: number } | null>(null);
  useEffect(() => {
    const screen = sessions.screen(key);
    if (!screen) {
      setState(null);
      return;
    }
    let frame = 0;
    const read = () => {
      frame = 0;
      const next = { ...screen.size(), ...screen.cursor() };
      setState((state) =>
        state && state.cols === next.cols && state.rows === next.rows && state.row === next.row && state.column === next.column
          ? state
          : next,
      );
    };
    read();
    const unsubscribe = screen.subscribe(() => {
      if (!frame) frame = requestAnimationFrame(read);
    });
    return () => {
      unsubscribe();
      cancelAnimationFrame(frame);
    };
  }, [sessions, key]);
  return state;
}

/** Whole seconds left until `at` (`Date.now()`), changing as each second passes; null without one. */
function useCountdown(at: number | null): number | null {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (at === null) return;
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      const current = Date.now();
      setNow(current);
      const left = at - current;
      // At the next whole second before `at`.
      if (left > 0) timer = setTimeout(tick, left % 1000 || 1000);
    };
    tick();
    return () => clearTimeout(timer);
  }, [at]);
  return at === null ? null : Math.max(1, Math.ceil((at - now) / 1000));
}

/** The time, updated every `interval` milliseconds. */
function useTick(interval: number): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), interval);
    return () => clearInterval(timer);
  }, [interval]);
  return now;
}
