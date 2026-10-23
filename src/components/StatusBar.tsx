import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { Profile } from "../ipc/api";
import type { Pane } from "../lib/panes";
import type { SessionRegistry, TerminalScreen } from "../lib/sessionRegistry";
import { address, serialSummary } from "../lib/sessions";
import { useSettings } from "../hooks/settings";
import { useRegistered } from "../hooks/stores";
import type { TerminalDetail, TrafficMeter } from "../lib/traffic";
import { TrafficLights, useTrafficText } from "./TrafficLights";

interface Props {
  /** The focused pane of the active tab. */
  pane: Pane;
  /** The pane's saved session; undefined for local terminals, quick connections and deleted sessions. */
  profile: Profile | undefined;
  sessions: SessionRegistry;
}

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
 * The lights for what the terminal sends and receives, with the channel it runs on and what
 * it has carried in the tooltip.
 */
function Activity({ meter }: { meter: TrafficMeter<TerminalDetail> | undefined }) {
  const { t } = useTranslation();
  const text = useTrafficText();
  const summary = () => {
    const channel = meter?.detail;
    return [
      ...(channel ? [text.channel(channel.connection, channel.channel)] : []),
      ...text.counts(meter?.counts),
      t("traffic.terminalScope"),
    ];
  };
  return <TrafficLights source={meter} summary={summary} />;
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
