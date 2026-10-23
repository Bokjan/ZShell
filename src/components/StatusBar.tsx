import { useEffect, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

import type { Profile } from "../ipc/api";
import type { Pane } from "../lib/panes";
import type { SessionRegistry, TerminalScreen } from "../lib/sessionRegistry";
import { address, serialSummary } from "../lib/sessions";
import { useMediaQuery, useSettings } from "../hooks/settings";
import type { TrafficMeter } from "../lib/traffic";
import { useLocale } from "../hooks/locale";
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
export function StatusBar(props: Props) {
  const { settings } = useSettings();
  return settings.statusBar.show ? <StatusItems {...props} /> : null;
}

function StatusItems({ pane, profile, sessions }: Props) {
  const { t } = useTranslation();
  const screen = useScreen(sessions, pane.key);
  const meter = useRegistered(sessions, () => sessions.get(pane.key)?.traffic);
  const connection = profile?.connection;
  const serial = connection?.protocol === "serial" ? connection : null;
  return (
    <footer className="status-bar" role="group" aria-label={t("statusBar.label")}>
      <Connection pane={pane} profile={profile} />
      <Activity meter={meter} />
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
  else if (pane.waiting !== null) state = t("tabs.status.waiting", { count: pane.waiting });
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
 * The send and receive lights, with what the connection has carried in the tooltip. Its text
 * is worked out each second, for "last received" (taken when the pointer arrives), and drawn
 * again only when it has changed.
 */
function Activity({ meter }: { meter: TrafficMeter | undefined }) {
  const { t } = useTranslation();
  const { formatSize, formatAgo } = useLocale();
  const reduced = useMediaQuery("(prefers-reduced-motion: reduce)");
  const { send, receive } = useLights(meter, reduced ? LIGHT_REDUCED_MS : LIGHT_MS);
  const summary = () => {
    const sent = formatSize(meter?.sent ?? 0);
    const received = formatSize(meter?.received ?? 0);
    const last = meter?.lastReceived;
    return [
      t("statusBar.traffic", { sent, received }),
      last == null ? t("statusBar.nothingReceived") : t("statusBar.lastReceived", { time: formatAgo(Date.now() - last) }),
    ].join("\n");
  };
  const text = useEverySecond(summary);
  const lines = text.split("\n");
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

/**
 * What the registry has for the pane (its session, its terminal), rendering again when that
 * comes or goes: a new pane's terminal attaches its session in an effect, after the status bar
 * has rendered, and a terminal shown again attaches a new one.
 */
function useRegistered<T>(sessions: SessionRegistry, read: () => T): T {
  return useSyncExternalStore(sessions.subscribe, read);
}

/** The pane's terminal size and cursor, at most once a frame; null until its terminal is shown. */
function useScreen(sessions: SessionRegistry, key: number) {
  const screen = useRegistered(sessions, () => sessions.screen(key));
  // Kept with the terminal it was read from, so that another one never shows it.
  const [state, setState] = useState<{ from: TerminalScreen; shown: { cols: number; rows: number; row: number; column: number } } | null>(
    null,
  );
  useEffect(() => {
    if (!screen) return;
    let frame = 0;
    const read = () => {
      frame = 0;
      const next = { ...screen.size(), ...screen.cursor() };
      setState((state) => {
        const shown = state?.from === screen ? state.shown : null;
        return shown && shown.cols === next.cols && shown.rows === next.rows && shown.row === next.row && shown.column === next.column
          ? state
          : { from: screen, shown: next };
      });
    };
    read();
    const unsubscribe = screen.subscribe(() => {
      if (!frame) frame = requestAnimationFrame(read);
    });
    return () => {
      unsubscribe();
      cancelAnimationFrame(frame);
    };
  }, [screen]);
  return screen && state?.from === screen ? state.shown : null;
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

/** What `compute` returns, worked out again every second: a render only when it changes. */
function useEverySecond(compute: () => string): string {
  const [value, setValue] = useState(compute);
  useEffect(() => {
    const update = () => setValue(compute());
    update();
    const timer = setInterval(update, 1000);
    return () => clearInterval(timer);
  }, [compute]);
  return value;
}
