import { Fragment, memo, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";

import type { Profile } from "../lib/api";
import { DialogsHidden } from "../lib/dialogs";
import type { MenuItem } from "./ContextMenu";
import { dragHorizontally, dragSplitter } from "../lib/drag";
import { dividers, equalize, MIN_PANE_HEIGHT, MIN_PANE_WIDTH, moveDivider, paneRects, type Divider, type Size } from "../lib/layout";
import { focusedPane, tabId, tabPanelId, type Layout, type Pane, type SidePanel, type Tab } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { ForwardsPanel } from "./ForwardsPanel";
import { Separator } from "./Separator";
import { SftpPanel } from "./SftpPanel";
import { TerminalView, type PasteTarget } from "./TerminalView";

/** What a tab page reports about its panes, and asks for them; each by pane key. */
export interface PaneHandlers {
  /** The panes' sessions. */
  sessions: SessionRegistry;
  onTitle(key: number, title: string): void;
  /** What the user typed in the pane's terminal (see `TerminalView`). */
  onInput(key: number, data: string): void;
  registerPaste(key: number, target: PasteTarget | null): void;
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
  /** Added to the end of the pane's terminal menu, when it opens. */
  menuItems(pane: Pane): MenuItem[];
  /** The pane's saved session; undefined for local terminals and deleted sessions. */
  profileOf(pane: Pane): Profile | undefined;
  onProfileChanged(profile: Profile): void;
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
  /** The saved sessions, which the panes' appearance and panels come from (see `profileOf`). */
  profiles: Profile[];
  /** Stable: the page renders again only when one of the other props changes. */
  handlers: PaneHandlers;
}

const MIN_PANEL = 280;
const MIN_TERMINAL = 240;

const percent = (fraction: number) => `${fraction * 100}%`;

/**
 * One tab's content: its panes, each a terminal, plus a side panel (files or port forwards)
 * on the focused pane's SSH connection.
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
    a.profiles === b.profiles &&
    a.handlers === b.handlers
  );
});

function TabPageView({ tab, active, syncing, inScope, flashing, refused, handlers: h }: Props) {
  const { t } = useTranslation();
  const [panelWidth, setPanelWidth] = useState(420);
  // Keep each pane's panels mounted once opened, so their state (directory, transfers)
  // survives switching panels and panes.
  const [mounted, setMounted] = useState<Record<number, SidePanel[]>>({});
  const pageRef = useRef<HTMLDivElement>(null);
  const areaRef = useRef<HTMLDivElement>(null);
  const tabRef = useRef(tab);
  tabRef.current = tab;
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
  const focused = focusedPane(tab);
  const panelHere = tab.sidePanel !== null && focused.protocol === "ssh";
  if (panelHere && !mounted[focused.key]?.includes(tab.sidePanel!)) {
    setMounted({ ...mounted, [focused.key]: [...(mounted[focused.key] ?? []), tab.sidePanel!] });
  }

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

  const panels = (pane: Pane) => {
    const shown = (panel: SidePanel) => pane.key === focused.key && tab.sidePanel === panel;
    const connected = pane.status === "connected" && pane.sessionId != null;
    const list = mounted[pane.key] ?? [];
    const profile = h.profileOf(pane);
    return (
      <Fragment key={pane.key}>
        {list.includes("files") && (
          <div className="side-panel-page" style={{ display: shown("files") ? undefined : "none" }}>
            <DialogsHidden.Provider value={!active || !shown("files")}>
              <SftpPanel
                sessionId={pane.sessionId}
                connected={connected}
                active={active && shown("files")}
                onTransfers={(count) => h.onTransfers(pane.key, count)}
              />
            </DialogsHidden.Provider>
          </div>
        )}
        {list.includes("forwards") && (
          <div className="side-panel-page" style={{ display: shown("forwards") ? undefined : "none" }}>
            <DialogsHidden.Provider value={!active || !shown("forwards")}>
              <ForwardsPanel
                sessionId={pane.sessionId}
                connected={connected}
                profile={profile}
                quick={pane.target.kind === "quick"}
                states={pane.forwards}
                onProfileChanged={h.onProfileChanged}
              />
            </DialogsHidden.Provider>
          </div>
        )}
      </Fragment>
    );
  };

  const anyMounted = tab.panes.some((pane) => (mounted[pane.key] ?? []).length > 0);
  // For where the side panel's edge is, as screen readers are told; read when rendering.
  const pageWidth = pageRef.current?.clientWidth ?? 0;

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
          {tab.panes.map((pane) => {
            const profile = h.profileOf(pane);
            const rect = rects.get(pane.key) ?? { x: 0, y: 0, w: 1, h: 1 };
            return (
              <div
                key={pane.key}
                data-pane={pane.key}
                className={[
                  "pane",
                  pane.key === focused.key && "focused",
                  syncing === pane.key && "syncing",
                  inScope.includes(pane.key) && "in-scope",
                ]
                  .filter(Boolean)
                  .join(" ")}
                style={{ left: percent(rect.x), top: percent(rect.y), width: percent(rect.w), height: percent(rect.h) }}
                onMouseDownCapture={() => h.onFocus(pane.key)}
                onFocus={() => h.onFocus(pane.key)}
              >
                <TerminalView
                  paneKey={pane.key}
                  sessions={h.sessions}
                  visible={active}
                  active={active && pane.key === focused.key}
                  onTitle={(title) => h.onTitle(pane.key, title)}
                  onInput={(data) => h.onInput(pane.key, data)}
                  registerPaste={(target) => h.registerPaste(pane.key, target)}
                  pasteTargets={() => h.pasteTargets(pane.key)}
                  menuItems={() => h.menuItems(pane)}
                  appearance={profile?.appearance}
                />
                {split && flashing.includes(pane.key) && <div className="pane-flash" />}
                {refused?.key === pane.key && <div key={refused.count} className="pane-refused" />}
              </div>
            );
          })}
        </div>
        {(anyMounted || tab.sidePanel) && (
          <div className="side-panel" style={{ width: panelWidth, display: tab.sidePanel ? undefined : "none" }}>
            <Separator
              className="splitter"
              label={t("tabs.panelDivider")}
              orientation="vertical"
              value={pageWidth > 0 ? ((pageWidth - panelWidth) / pageWidth) * 100 : 50}
              onMove={nudgePanel}
              onMouseDown={startResize}
            />
            {tab.panes.map(panels)}
            {tab.sidePanel && !panelHere && <div className="side-panel-empty">{t("tabs.panelUnavailable")}</div>}
          </div>
        )}
      </div>
    </DialogsHidden.Provider>
  );
}
