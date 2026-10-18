import { listenHere } from "../ipc/events";
import { tabDrag, type DropTarget } from "../ipc/windows";

/** What a `WindowDrag` tells the drag it is part of. */
export interface WindowDragEvents {
  /** Where the dragged tab or pane would go if dropped now, as the backend sees it. */
  target(target: DropTarget): void;
  /**
   * The system's drag took over (Wayland): the page hears no more of the mouse until the end
   * of that drag, `dropped` or cancelled (Escape).
   */
  native(): void;
  nativeEnd(dropped: boolean): void;
}

/**
 * The part of dragging a tab or a pane that may take it out of the window: while it moves,
 * the backend is asked where it would go (another window shows where in its tab bar), and
 * when it ends, where it goes. One question at a time, and the latest position after it.
 */
export class WindowDrag {
  /** Names the drag for the backend, which ignores answers that come back after it ended. */
  private readonly id = crypto.randomUUID();
  private asking = false;
  private askAgain = false;
  /** Where the pointer last was: outside the page or not. */
  private outside = false;
  private ended = false;
  private stopNative: Promise<() => void> | null = null;

  constructor(private readonly events: WindowDragEvents) {}

  /** The pointer moved, `outside` the page or not. */
  move(outside: boolean) {
    this.outside = outside;
    if (this.asking) {
      this.askAgain = true;
      return;
    }
    this.asking = true;
    void tabDrag
      .move(this.id, outside)
      .then((target) => {
        if (this.ended) return;
        if (target.kind === "native") this.goNative();
        this.events.target(target);
      }, console.error)
      .finally(() => {
        this.asking = false;
        if (this.askAgain && !this.ended) {
          this.askAgain = false;
          this.move(this.outside);
        }
      });
  }

  /**
   * Ends the drag, `dropped` (`outside` the page, if known) or not: where it goes if it leaves
   * the window, else null. The backend hears of it either way, which takes down the marker
   * another window may show.
   */
  end(dropped: boolean, outside = this.outside): Promise<DropTarget | null> {
    if (this.ended) return Promise.resolve(null);
    this.ended = true;
    void this.stopNative?.then((stop) => stop());
    this.stopNative = null;
    return tabDrag.end(this.id, dropped, outside);
  }

  private goNative() {
    if (this.stopNative || this.ended) return;
    this.stopNative = listenHere("tab-drag-native-end", ({ payload }) => {
      if (payload.id !== this.id) return;
      void this.stopNative?.then((stop) => stop());
      this.stopNative = null;
      this.events.nativeEnd(payload.dropped);
    });
    this.events.native();
  }
}
