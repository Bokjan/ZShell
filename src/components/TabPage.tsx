import { memo, useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";

import { DialogsHidden } from "../hooks/dialogs";
import { useLatest } from "../hooks/latest";
import { useStableProps } from "../hooks/stableProps";
import type { MenuItem } from "./ContextMenu";
import { dragHorizontally, dragSplitter } from "../hooks/drag";
import {
  dividers,
  equalize,
  MIN_PANE_HEIGHT,
  MIN_PANE_WIDTH,
  moveDivider,
  paneRects,
  type Divider,
  type Rect,
  type Size,
} from "../lib/layout";
import { focusedPane, tabId, tabPanelId, type Layout, type Pane, type Tab } from "../lib/panes";
import type { PasteTarget, SessionRegistry } from "../lib/sessionRegistry";
import { GripIcon } from "./icons";
import { holdOnly, place, type PaneHosts } from "./paneHosts";
import { Separator } from "./Separator";

/** What a tab page reports about its panes, and asks for them; each by pane key. */
export interface PaneHandlers {
  /** The panes' sessions. */
  sessions: SessionRegistry;
  /** Where the panes' terminals and side panels are drawn (see `PaneHostList`). */
  hosts: PaneHosts;
  onTitle(key: number, title: string): void;
  /** What the user typed in the pane's terminal (see `TerminalView`). */
  onInput(key: number, data: string): void;
  /** The other panes a paste in the pane goes to as well (syncing). */
  pasteTargets(key: number): PasteTarget[];
  /** The pane was clicked or got the keyboard focus. */
  onFocus(key: number): void;
  /** The tab's panes were resized. */
  onLayout(tabKey: number, layout: Layout): void;
  /** The size of the tab's pane area changed (the window or the side panel was resized). */
  onAreaSize(tabKey: number, size: Size): void;
  /** How many uploads and downloads are running in the pane's file panel. */
  onTransfers(key: number, count: number): void;
  /** How many files of the pane's file panel are open in a local editor. */
  onEdits(key: number, count: number): void;
  /** How many questions are waiting in the pane's file panel. */
  onQuestions(key: number, count: number): void;
  /** Added to the end of the pane's terminal menu, when it opens. */
  menuItems(key: number): MenuItem[];
  /** The pane's grip was pressed: it may be dragged elsewhere (see `usePaneDrag`). */
  onGrab(e: MouseEvent, key: number): void;
}

interface Props {
  tab: Tab;
  active: boolean;
  /** The pane whose input is sent to other panes too (the compose bar's sync); marked. */
  syncing: number | null;
  /** Panes that the compose bar sends to besides (or instead of) the focused one; marked when split. */
  inScope: number[];
  /** Panes that just received text from the compose bar or a quick command; they flash. */
  flashing: number[];
  /** A pane that couldn't be split; its border flashes, anew for each `count`. */
  refused: { key: number; count: number } | null;
  /** The panes have somewhere else to go (the tab is split, or there are other tabs or windows): they have grips. */
  draggable: boolean;
  /** Stable: the page renders again only when one of the other props changes. */
  handlers: PaneHandlers;
}

const MIN_PANEL = 280;
const MIN_TERMINAL = 240;

const percent = (fraction: number) => `${fraction * 100}%`;

const FULL: Rect = { x: 0, y: 0, w: 1, h: 1 };

/**
 * One tab's content: its panes, each a terminal, plus a side panel (files or port forwards)
 * on the focused pane's SSH connection. The terminals and panels themselves are drawn
 * elsewhere (`PaneHostList`) into hosts that the page puts in place.
 */
export const TabPage = memo(TabPageView, (a, b) => {
  const same = (x: number[], y: number[]) => x.length === y.length && x.every((key, i) => key === y[i]);
  return (
    a.tab === b.tab &&
    a.active === b.active &&
    a.syncing === b.syncing &&
    same(a.inScope, b.inScope) &&
    same(a.flashing, b.flashing) &&
    a.refused === b.refused &&
    a.draggable === b.draggable &&
    a.handlers === b.handlers
  );
});

function TabPageView(props: Props) {
  useStableProps("TabPage", props, ["handlers"]);
  const { tab, active, syncing, inScope, flashing, refused, draggable, handlers: h } = props;
  const { t } = useTranslation();
  const [panelWidth, setPanelWidth] = useState(420);
  const pageRef = useRef<HTMLDivElement>(null);
  const areaRef = useRef<HTMLDivElement>(null);
  const panelPagesRef = useRef<HTMLDivElement>(null);
  const tabRef = useLatest(tab);
  const split = tab.panes.length > 1;
  const rects = paneRects(tab.layout);

  // For whether a pane has room to split (see `canSplit`).
  useEffect(() => {
    const area = areaRef.current!;
    const observer = new ResizeObserver(() => {
      const { width, height } = area.getBoundingClientRect();
      if (width > 0 && height > 0) h.onAreaSize(tab.key, { width, height });
    });
    observer.observe(area);
    return () => observer.disconnect();
  }, [h, tab.key]);

  // For where the side panel's edge is, as screen readers are told: only while it is shown,
  // or every tab would render again on each frame of resizing the window.
  const [pageWidth, setPageWidth] = useState(0);
  const panelShown = active && tab.sidePanel !== null;
  useEffect(() => {
    if (!panelShown) return;
    const page = pageRef.current!;
    const observer = new ResizeObserver(() => setPageWidth(page.clientWidth));
    observer.observe(page);
    return () => observer.disconnect();
  }, [panelShown]);
  const focused = focusedPane(tab);
  const panelHere = tab.sidePanel !== null && focused.protocol === "ssh";

  // The panes' side panels, each pane's shown while it has the focus (see `PaneHost`). Only
  // the hosts of panes that come or go move: a split leaves the open panel as it was. Those
  // still here when the page goes go with it, and a pane moving on takes its host along.
  const paneKeys = tab.panes.map((pane) => pane.key).join(" ");
  useLayoutEffect(() => {
    const keys = paneKeys.split(" ").map(Number);
    holdOnly(
      panelPagesRef.current!,
      keys.map((key) => h.hosts.panels(key)),
    );
  }, [h, paneKeys]);

  const startResize = (e: ReactMouseEvent) => {
    const rect = pageRef.current!.getBoundingClientRect();
    dragHorizontally(e, (x) => setPanelWidth(Math.max(MIN_PANEL, Math.min(rect.right - x, rect.width - MIN_TERMINAL))));
  };

  /** The divider's split on screen along its direction, and the smallest size of a pane next to it (a fraction of the split). */
  const splitExtent = (divider: Divider) => {
    const area = areaRef.current!.getBoundingClientRect();
    const row = divider.direction === "row";
    const start = row ? area.left + divider.rect.x * area.width : area.top + divider.rect.y * area.height;
    const length = row ? divider.rect.w * area.width : divider.rect.h * area.height;
    return { start, length, min: (row ? MIN_PANE_WIDTH : MIN_PANE_HEIGHT) / length };
  };

  const startDivider = (e: ReactMouseEvent, divider: Divider) => {
    const { start, length, min } = splitExtent(divider);
    dragSplitter(e, divider.direction === "row" ? "x" : "y", (position) =>
      h.onLayout(tab.key, moveDivider(tabRef.current.layout, divider, (position - start) / length, min)),
    );
  };

  // From the keyboard: from where the divider is in the latest layout, as keys can repeat
  // faster than the page renders.
  const nudgeDivider = (divider: Divider, pixels: number) => {
    const same = (d: Divider) => d.index === divider.index && d.path.join(".") === divider.path.join(".");
    const layout = tabRef.current.layout;
    const current = dividers(layout).find(same) ?? divider;
    const { length, min } = splitExtent(current);
    h.onLayout(tab.key, moveDivider(layout, current, current.at + pixels / length, min));
  };

  const nudgePanel = (pixels: number) => {
    const page = pageRef.current!.getBoundingClientRect();
    setPanelWidth((width) => Math.max(MIN_PANEL, Math.min(width - pixels, page.width - MIN_TERMINAL)));
  };

  // Dialogs of an inactive tab (a background save's conflict) wait for it to be shown.
  return (
    <DialogsHidden.Provider value={!active}>
      <div
        className={`tab-page${active ? " active" : ""}`}
        ref={pageRef}
        id={tabPanelId(tab.key)}
        role="tabpanel"
        aria-labelledby={tabId(tab.key)}
      >
        <div className={`pane-area${split ? " split" : ""}${syncing !== null ? " syncing" : ""}`} ref={areaRef}>
          {/* Before the panes, for Tab: a terminal keeps Tab for the shell. */}
          {dividers(tab.layout).map((divider) => {
            const { rect, at } = divider;
            const row = divider.direction === "row";
            const style = row
              ? { left: percent(rect.x + at * rect.w), top: percent(rect.y), height: percent(rect.h) }
              : { top: percent(rect.y + at * rect.h), left: percent(rect.x), width: percent(rect.w) };
            return (
              <Separator
                key={`${divider.path.join(".")}:${divider.index}`}
                className={`pane-divider ${divider.direction}`}
                style={style}
                label={t("tabs.paneDivider")}
                orientation={row ? "vertical" : "horizontal"}
                value={at * 100}
                onMove={(pixels) => nudgeDivider(divider, pixels)}
                onReset={() => h.onLayout(tab.key, equalize(tabRef.current.layout, divider.path))}
                onMouseDown={(e) => startDivider(e, divider)}
              />
            );
          })}
          {/* Flat, in the order opened, and placed by the layout's percentages: splitting,
              closing or resizing never remounts another pane's terminal, which would end its
              session. */}
          {tab.panes.map((pane) => (
            <PaneItem
              key={pane.key}
              pane={pane}
              rect={rects.get(pane.key) ?? FULL}
              focused={pane.key === focused.key}
              syncing={syncing === pane.key}
              inScope={inScope.includes(pane.key)}
              flashing={split && flashing.includes(pane.key)}
              refused={refused?.key === pane.key ? refused.count : null}
              draggable={draggable}
              handlers={h}
            />
          ))}
        </div>
        {/* Always there, for the panes' panels that stay mounted while out of sight. */}
        <div className="side-panel" style={{ width: panelWidth, display: tab.sidePanel ? undefined : "none" }}>
          <Separator
            className="splitter"
            label={t("tabs.panelDivider")}
            orientation="vertical"
            value={pageWidth > 0 ? ((pageWidth - panelWidth) / pageWidth) * 100 : 50}
            onMove={nudgePanel}
            onMouseDown={startResize}
          />
          <div className="pane-host" ref={panelPagesRef} />
          {tab.sidePanel && !panelHere && <div className="side-panel-empty">{t("tabs.panelUnavailable")}</div>}
        </div>
      </div>
    </DialogsHidden.Provider>
  );
}

interface PaneItemProps {
  pane: Pane;
  /** Where the pane is in the tab's area, as fractions of it. */
  rect: Rect;
  focused: boolean;
  syncing: boolean;
  inScope: boolean;
  flashing: boolean;
  /** Each time the pane couldn't be split, a new count. */
  refused: number | null;
  /** It has a grip to drag it by. */
  draggable: boolean;
  handlers: PaneHandlers;
}

/** How near the pane's top edge the pointer brings up its grip, in CSS pixels. */
const GRIP_ZONE = 24;

/**
 * One pane's place in the tab, with its marks; its own component so that React Compiler
 * memoizes it. Its terminal is drawn into a host (see `PaneHost`), put here as the pane
 * comes: React's events from it go to `PaneHost`, so the focus is followed with the DOM's.
 */
function PaneItem(props: PaneItemProps) {
  useStableProps("PaneItem", props, ["handlers"]);
  const { pane, rect, focused, syncing, inScope, flashing, refused, draggable, handlers: h } = props;
  const { t } = useTranslation();
  const { key } = pane;
  const ref = useRef<HTMLDivElement>(null);
  // The pointer is near the top edge, not pressing a button (selecting text, say).
  const [near, setNear] = useState(false);

  useLayoutEffect(() => {
    const div = ref.current!;
    const focus = () => h.onFocus(key);
    div.addEventListener("mousedown", focus, true);
    div.addEventListener("focusin", focus);
    const unplace = place(div, [h.hosts.terminal(key)]);
    return () => {
      div.removeEventListener("mousedown", focus, true);
      div.removeEventListener("focusin", focus);
      unplace();
    };
  }, [h, key]);

  // Where the top edge is, measured as the pointer comes in or the pane moves, rather than on
  // each move of the pointer over the terminal.
  useEffect(() => {
    if (!draggable) return;
    const div = ref.current!;
    let top = div.getBoundingClientRect().top;
    let shown = false;
    const show = (value: boolean) => {
      if (value === shown) return;
      shown = value;
      setNear(value);
    };
    const enter = () => void (top = div.getBoundingClientRect().top);
    const move = (e: MouseEvent) => show(e.buttons === 0 && e.clientY - top < GRIP_ZONE);
    const leave = () => show(false);
    div.addEventListener("mouseenter", enter);
    div.addEventListener("mousemove", move, { passive: true });
    div.addEventListener("mouseleave", leave);
    return () => {
      div.removeEventListener("mouseenter", enter);
      div.removeEventListener("mousemove", move);
      div.removeEventListener("mouseleave", leave);
    };
  }, [draggable, rect.y]);

  return (
    <div
      ref={ref}
      data-pane={key}
      className={["pane", focused && "focused", syncing && "syncing", inScope && "in-scope"].filter(Boolean).join(" ")}
      style={{ left: percent(rect.x), top: percent(rect.y), width: percent(rect.w), height: percent(rect.h) }}
    >
      {draggable && (
        // Only for the pointer: the terminal's menu moves the pane from the keyboard.
        <div
          className={`pane-grip${near ? " near" : ""}`}
          aria-hidden="true"
          title={t("tabs.dragPane")}
          onMouseDown={(e) => h.onGrab(e.nativeEvent, key)}
        >
          <GripIcon />
        </div>
      )}
      {flashing && <div className="pane-flash" />}
      {refused !== null && <div key={refused} className="pane-refused" />}
    </div>
  );
}
