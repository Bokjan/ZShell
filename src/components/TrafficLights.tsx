import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ConnectionInfo, Counts } from "../ipc/api";
import { useMediaQuery } from "../hooks/settings";
import { useLocale } from "../hooks/locale";
import { channelRanges, NO_COUNTS, type TrafficSource } from "../lib/traffic";
import { ActivityIcon } from "./icons";

/**
 * How long a light stays on after data was sent or received: steady while data keeps coming,
 * as the backend reports at least every 50 ms then. Longer when the system asks for less
 * motion, so it blinks less.
 */
const LIGHT_MS = 150;
const LIGHT_REDUCED_MS = 400;
/** How often the rates and the tooltip are worked out again. */
const TEXT_MS = 500;

interface Props {
  source: TrafficSource | undefined;
  /** The lines of the tooltip, which screen readers read too: worked out again and again, for "last received". */
  summary: () => string[];
  /** Also shows what is sent and received a second, after each light. */
  rates?: boolean;
  className?: string;
}

/**
 * The lights for data sent and received through a connection's channels (see `lib/traffic`),
 * with the rates after them if asked. Its text is worked out every `TEXT_MS` and drawn again
 * only when it has changed. Not a live region: the rates change all the time. While hidden (in a
 * side panel not shown, which stays mounted) it is left alone, and brought up to date when
 * shown again.
 */
export function TrafficLights({ source, summary, rates = false, className }: Props) {
  const { t } = useTranslation();
  const { formatSize } = useLocale();
  const reduced = useMediaQuery("(prefers-reduced-motion: reduce)");
  const [element, setElement] = useState<HTMLSpanElement | null>(null);
  const shown = useShown(element);
  const { send, receive } = useLights(shown ? source : undefined, reduced ? LIGHT_REDUCED_MS : LIGHT_MS);
  const text = useRecomputed(shown ? TEXT_MS : null, () => {
    const rate = source?.rate() ?? { sent: 0, received: 0 };
    const sent = t("traffic.rate", { size: formatSize(Math.round(rate.sent)) });
    const received = t("traffic.rate", { size: formatSize(Math.round(rate.received)) });
    const label = rates ? [t("traffic.rateLabel", { sent, received })] : [];
    return [sent, received, ...label, ...summary()].join("\n");
  });
  const [sent, received, ...lines] = text.split("\n");
  const tooltip = rates ? lines.slice(1) : lines;
  return (
    <span ref={setElement} className={`traffic-lights${className ? ` ${className}` : ""}`} title={tooltip.join("\n")}>
      <span className={send ? "send on" : "send"}>
        <ActivityIcon direction="up" />
      </span>
      {rates && (
        <span className="traffic-rate" aria-hidden="true">
          {sent}
        </span>
      )}
      <span className={receive ? "receive on" : "receive"}>
        <ActivityIcon direction="down" />
      </span>
      {rates && (
        <span className="traffic-rate" aria-hidden="true">
          {received}
        </span>
      )}
      <span className="visually-hidden">{lines.join(" ")}</span>
    </span>
  );
}

/** The sentences that describe traffic, in the interface language. */
export function useTrafficText() {
  const { t } = useTranslation();
  const { formatSize, formatAgo, formatList } = useLocale();
  const connection = ({ id, localPort }: ConnectionInfo) =>
    localPort === null ? t("traffic.connection", { id }) : t("traffic.connectionWithPort", { id, port: localPort });
  return {
    /** What was sent and received, and when something last came. */
    counts: (counts: Counts = NO_COUNTS) => [
      t("traffic.counts", { sent: formatSize(counts.sent), received: formatSize(counts.received) }),
      counts.lastReceived === null
        ? t("traffic.nothingReceived")
        : t("traffic.lastReceived", { time: formatAgo(Date.now() - counts.lastReceived) }),
    ],
    channel: (on: ConnectionInfo, channel: number) => t("traffic.channel", { connection: connection(on), channel }),
    /** `channels`: the first of `count` listed. */
    channels: (on: ConnectionInfo, channels: number[], count: number) => {
      if (count === 0) return t("traffic.noChannels", { connection: connection(on) });
      const more = count > channels.length ? [t("traffic.moreChannels", { count: count - channels.length })] : [];
      return t("traffic.channels", { connection: connection(on), channels: formatList([...channelRanges(channels), ...more]), count });
    },
  };
}

/** Whether data was sent and received within the last `hold` milliseconds, at most once a frame. */
function useLights(source: TrafficSource | undefined, hold: number) {
  const [lights, setLights] = useState({ send: false, receive: false });
  useEffect(() => {
    let frame = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const update = () => {
      frame = 0;
      clearTimeout(timer);
      const now = Date.now();
      const until = (at: number | null | undefined) => (at == null ? 0 : at + hold - now);
      const send = until(source?.counts.lastSent);
      const receive = until(source?.counts.lastReceived);
      setLights((lights) =>
        lights.send === send > 0 && lights.receive === receive > 0 ? lights : { send: send > 0, receive: receive > 0 },
      );
      // Off again once the newer of the two has had its time.
      const next = Math.min(...[send, receive].filter((left) => left > 0));
      if (Number.isFinite(next)) timer = setTimeout(update, next);
    };
    update();
    const unsubscribe = source?.subscribe(() => {
      if (!frame) frame = requestAnimationFrame(update);
    });
    return () => {
      unsubscribe?.();
      cancelAnimationFrame(frame);
      clearTimeout(timer);
    };
  }, [source, hold]);
  return lights;
}

/**
 * Whether `element` is shown, rather than in something hidden with `display: none`: its size
 * changes to nothing and back as that comes and goes. It must be a box (`.traffic-lights` is
 * `inline-flex`): a plain inline element always has no size for a `ResizeObserver`.
 */
function useShown(element: Element | null): boolean {
  const [shown, setShown] = useState(true);
  useEffect(() => {
    if (!element) return;
    const observer = new ResizeObserver(() => setShown(element.checkVisibility()));
    observer.observe(element);
    return () => observer.disconnect();
  }, [element]);
  return shown;
}

/**
 * What `compute` returns, worked out again every `interval` ms (and not at all while that is
 * null, until it is not): a render only when it changes.
 */
function useRecomputed(interval: number | null, compute: () => string): string {
  const [value, setValue] = useState(compute);
  useEffect(() => {
    if (interval === null) return;
    const update = () => setValue(compute());
    update();
    const timer = setInterval(update, interval);
    return () => clearInterval(timer);
  }, [interval, compute]);
  return value;
}
