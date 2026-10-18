import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import type { Point } from "../ipc/bindings";
import { halfOn, sideAt, type Rect } from "../lib/layout";
import { findPane, type Layout } from "../lib/panes";
import { createStore, type PatchStore } from "../lib/store";
import type { PanePlace, PaneSide, TabStore } from "../lib/tabs";
import type { TabRest } from "../lib/tabRest";
import { tabIndexAt, tabStrip } from "../lib/tabStrip";
import { WindowDrag } from "../lib/windowDrag";
import { DRAG_THRESHOLD } from "../components/useTabDrag";
import { leavesSomething, type useWindows } from "./useWindows";
import type { usePaneActions } from "./usePaneActions";

/** What the page shows of a drag, in CSS pixels of the page. */
interface DragView {
  /** The pane being dragged, dimmed while it is in sight. */
  source: Rect | null;
  /** Where what is dragged would go if dropped now: half of a pane. */
  preview: Rect | null;
  /** Where in the tab strip a new tab would go. */
  marker: Rect | null;
  /** Dropped now, the pane would leave the window. */
  leaving: boolean;
}

const NO_VIEW: DragView = { source: null, preview: null, marker: null, leaving: false };

/** Where a dragged pane goes if dropped now: see `PanePlace`, or out of the window. */
type Drop = PanePlace | { kind: "out" } | null;

const rectOf = (element: Element): Rect => {
  const r = element.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
};

/**
 * Dragging panes by their grips (`start`): to a tab of their own (dropped in the tab strip),
 * next to another pane of the tab shown (resting on another tab first shows that one), or out
 * of the window, as tabs go. Tabs dragged from the tab bar over the panes of the tab shown
 * join them through `overPanes` and `dropOnPanes`. `layer` shows where they would go.
 */
export function usePaneDrag(
  store: TabStore,
  actions: Pick<ReturnType<typeof usePaneActions>, "fitsBeside" | "movePane" | "mergeTab">,
  windowing: Pick<ReturnType<typeof useWindows>, "canMove" | "dropTab">,
  tabRest: TabRest,
) {
  const [view] = useState(() => createStore<DragView>(NO_VIEW));
  /** Where a tab dragged over the panes would join them. */
  const tabTarget = useRef<PaneSide | null>(null);
  /** Stops the drag going on, if any. */
  const stopRef = useRef<(() => void) | null>(null);
  useEffect(() => () => stopRef.current?.(), []);

  /**
   * The pane of the tab shown at (`x`, `y`), and the side of it nearest the point, if panes
   * arranged as `added` fit there; `moving` leaves its place first, and isn't a target itself.
   */
  const paneAt = (x: number, y: number, added: Layout, moving?: number): { to: PaneSide; preview: Rect } | null => {
    const element = document.elementFromPoint(x, y)?.closest<HTMLElement>(".tab-page.active .pane[data-pane]");
    const key = Number(element?.dataset.pane);
    if (!element || key === moving) return null;
    const rect = rectOf(element);
    const side = sideAt(rect, x, y);
    const to = { key, side };
    return actions.fitsBeside(added, to, moving) ? { to, preview: halfOn(rect, side) } : null;
  };

  const overPanes = (key: number, at: Point | null) => {
    const tab = store.get().tabs.find((t) => t.key === key);
    const found = at && tab && tab.key !== store.get().activeKey && !tab.moving ? paneAt(at.x, at.y, tab.layout) : null;
    tabTarget.current = found?.to ?? null;
    view.set({ preview: found?.preview ?? null });
    return found !== null;
  };

  const dropOnPanes = (key: number) => {
    const to = tabTarget.current;
    tabTarget.current = null;
    view.set(NO_VIEW);
    if (to) actions.mergeTab(key, to);
  };

  const start = (e: MouseEvent, key: number) => {
    if (e.button !== 0) return;
    stopRef.current?.();
    e.preventDefault();
    const grab = { x: e.clientX, y: e.clientY };
    const startTab = store.get().activeKey;
    const leaf: Layout = { kind: "pane", key };
    let moved = false;
    let ended = false;
    // Whether it can go to another window, as the drag starts.
    let movable = false;
    let outside = false;
    let drop: Drop = null;
    let frame = 0;
    let last: MouseEvent | null = null;
    const tabOfPane = () => findPane(store.get().tabs, key)?.tab;
    const isOutside = (ev: MouseEvent) =>
      ev.clientX < 0 || ev.clientY < 0 || ev.clientX >= window.innerWidth || ev.clientY >= window.innerHeight;

    const drag = new WindowDrag({
      target: (target) => {
        const tab = tabOfPane();
        const leaves = !!tab && (target.kind === "window" || (target.kind !== "here" && leavesSomething(store.get().tabs, tab.key, key)));
        if (!ended) view.set({ leaving: leaves });
      },
      native: () => removeListeners(),
      nativeEnd: (dropped) => finish(dropped),
    });

    /** Where the pane would go with the pointer at (`x`, `y`), and what shows it. */
    const update = (x: number, y: number) => {
      const tab = tabOfPane();
      if (!tab) return finish(false);
      const strip = tabStrip();
      const bounds = strip?.getBoundingClientRect();
      const source = document.querySelector(`.tab-page.active .pane[data-pane="${key}"]`);
      let preview: Rect | null = null;
      let marker: Rect | null = null;
      if (outside) {
        drop = movable ? { kind: "out" } : null;
        tabRest.rest(null);
      } else if (strip && bounds && y >= bounds.top && y <= bounds.bottom) {
        const over = document.elementFromPoint(x, y)?.closest<HTMLElement>(".tab");
        tabRest.rest(over ? Number(over.dataset.key) : null);
        // A tab's only pane is a tab of its own already.
        const index = tabIndexAt(strip, x);
        drop = tab.panes.length > 1 ? { kind: "tab", index } : null;
        if (drop) {
          const elements = [...strip.querySelectorAll(".tab")];
          const lastTab = elements[elements.length - 1];
          const edge =
            index < elements.length
              ? elements[index].getBoundingClientRect().left
              : (lastTab?.getBoundingClientRect().right ?? bounds.left);
          marker = { x: edge - 1, y: bounds.top + 4, w: 2, h: bounds.height - 8 };
        }
      } else {
        tabRest.rest(null);
        const found = paneAt(x, y, leaf, key);
        drop = found && { kind: "pane", ...found.to };
        preview = found?.preview ?? null;
      }
      view.set({ source: source && rectOf(source), preview, marker });
    };

    const move = (ev: MouseEvent) => {
      outside = isOutside(ev);
      // Released where the page didn't hear it.
      if (ev.buttons === 0) {
        finish(true);
        return;
      }
      if (!moved) {
        if (Math.hypot(ev.clientX - grab.x, ev.clientY - grab.y) < DRAG_THRESHOLD) return;
        moved = true;
        const tab = tabOfPane();
        movable = !!tab && windowing.canMove(tab, key);
        document.body.classList.add("dragging-pane");
      }
      if (movable) drag.move(outside);
      // At most once a frame, at the latest position.
      last = ev;
      if (frame === 0) {
        frame = requestAnimationFrame(() => {
          frame = 0;
          if (last && !ended) update(last.clientX, last.clientY);
        });
      }
    };
    const keyDown = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      finish(false);
    };
    const up = (ev: MouseEvent) => {
      outside = isOutside(ev);
      // Where it was released, which may not have been looked at yet.
      if (moved && !outside) update(ev.clientX, ev.clientY);
      finish(true);
    };
    const removeListeners = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", keyDown, true);
    };
    const finish = (dropped: boolean) => {
      if (ended) return;
      ended = true;
      removeListeners();
      cancelAnimationFrame(frame);
      tabRest.rest(null);
      document.body.classList.remove("dragging-pane");
      view.set(NO_VIEW);
      stopRef.current = null;
      const target = dropped && moved ? drop : null;
      const tab = tabOfPane();
      if (target?.kind === "tab" || target?.kind === "pane") actions.movePane(key, target);
      // Not dropped anywhere: the tab it came from is shown again.
      else if (target === null && moved && startTab !== null) store.dispatch({ type: "activate", key: startTab });
      if (!movable) return;
      void drag.end(target?.kind === "out", outside).then((to) => to && tab && windowing.dropTab(tab.key, to, grab, key), console.error);
    };

    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", keyDown, true);
    stopRef.current = () => finish(false);
  };

  return { start, overPanes, dropOnPanes, view };
}

/** Shows where a dragged pane or tab would go (see `usePaneDrag`). */
export function PaneDragLayer({ view }: { view: PatchStore<DragView> }) {
  const { source, preview, marker, leaving } = useSyncExternalStore(view.subscribe, view.get);
  const style = (rect: Rect) => ({ left: rect.x, top: rect.y, width: rect.w, height: rect.h });
  return (
    <>
      {source && <div className={`pane-drag-source${leaving ? " leaving" : ""}`} style={style(source)} aria-hidden="true" />}
      {preview && <div className="pane-drop-preview" style={style(preview)} aria-hidden="true" />}
      {marker && <div className="pane-drop-marker" style={style(marker)} aria-hidden="true" />}
    </>
  );
}
