import { WebglAddon } from "@xterm/addon-webgl";
import type { Terminal } from "@xterm/xterm";

/**
 * The pixels the glyph atlases may hold in all, about 64 MB, before the terminals drawing
 * with WebGL start over with new ones. An atlas keeps every glyph it has drawn, once for each
 * color and style it was drawn in, and grows by pages up to 4096 pixels square: with Chinese
 * text in many colors it keeps growing, by hundreds of MB over a long session. A screenful of
 * distinct wide glyphs, each in its own color, takes a tenth of this (150 by 40 cells at 175%)
 * and a third on a 4K screen at 200%: starting over never follows itself, and is rare.
 */
const ATLAS_PIXEL_LIMIT = 4096 * 4096;

/** The part of xterm.js's internals used here, which may be missing in another version. */
interface Internals {
  _core?: { _renderService?: { _renderRows?(start: number, end: number): void } };
}

interface Renderer {
  /** The atlas pages this terminal's addon has added and not yet removed. */
  pages: Set<HTMLCanvasElement>;
  drawing(): boolean;
  load(): void;
  release(): void;
}

/** How long after losing its context a terminal tries WebGL again, and how many times. */
const RETRY_MS = [1000, 5000, 30_000];

const renderers = new Set<Renderer>();
/** Releases put off until the effects of the commit that hid their terminals have run, by terminal. */
const pendingReleases = new Map<Terminal, () => void>();
let renewal: ReturnType<typeof setTimeout> | undefined;
/** Set once loading the addon failed: WebGL2 is unavailable, and trying again won't help. */
let unavailable = false;

/**
 * Atlases are shared by the terminals with the same font, colors and pixel ratio, and live
 * while a terminal uses them: all the terminals drawing with WebGL let theirs go before any
 * loads a new addon. Some pages may be missed (those an atlas had beyond its first before a
 * terminal joined it, once the terminal that saw them is gone), so the count errs low.
 */
function checkAtlases() {
  if (renewal !== undefined) return;
  const pages = new Set([...renderers].flatMap((renderer) => [...renderer.pages]));
  const pixels = [...pages].reduce((sum, page) => sum + page.width * page.height, 0);
  if (pixels <= ATLAS_PIXEL_LIMIT) return;
  // Not while the atlas is drawing a glyph, which is when it adds a page.
  renewal = setTimeout(() => {
    renewal = undefined;
    const drawing = [...renderers].filter((renderer) => renderer.drawing());
    drawing.forEach((renderer) => renderer.release());
    drawing.forEach((renderer) => renderer.load());
  }, 0);
}

/**
 * Draws the terminal with WebGL until the returned function is called, and with the DOM
 * renderer when WebGL is unavailable or while its context is lost (it is tried again a few
 * times: after the computer sleeps, or the system switches GPUs).
 */
export function drawWithWebgl(term: Terminal): () => void {
  // One addon at a time: a release still pending (hidden and shown again at once) goes first.
  pendingReleases.get(term)?.();
  let addon: WebglAddon | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let losses = 0;
  // The addon's context, while it is there to release.
  let gl: WebGL2RenderingContext | undefined;
  // Waits for the new canvas's first layout, to draw it then.
  let sized: ResizeObserver | undefined;
  const pages = new Set<HTMLCanvasElement>();
  const renderer: Renderer = {
    pages,
    drawing: () => addon !== undefined,
    load() {
      if (unavailable) return;
      let webgl: WebglAddon | undefined;
      try {
        webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          gl = undefined;
          renderer.release();
          const delay = RETRY_MS[losses++];
          if (delay === undefined) return;
          retry = setTimeout(() => {
            retry = undefined;
            if (renderers.has(renderer)) renderer.load();
          }, delay);
        });
        webgl.onAddTextureAtlasCanvas((page) => {
          pages.add(page);
          checkAtlases();
        });
        webgl.onRemoveTextureAtlasCanvas((page) => pages.delete(page));
        // Another atlas (new colors, font or pixel ratio): the old one's pages are no longer
        // this terminal's, and the atlas removes none of them as it goes.
        webgl.onChangeTextureAtlas((page) => {
          pages.clear();
          pages.add(page);
          checkAtlases();
        });
        const before = new Set(term.element?.querySelectorAll("canvas"));
        term.loadAddon(webgl);
        // The addon adds a 2D canvas for link underlines before its WebGL one. Asking a canvas
        // that has a 2D context for "webgl2" gives null, and creates nothing.
        const added = [...(term.element?.querySelectorAll("canvas") ?? [])].filter((c) => !before.has(c));
        gl = added.map((c) => c.getContext("webgl2")).find((context) => context !== null) ?? undefined;
        addon = webgl;
        // The new canvas is empty, and the terminal shows nothing until it is drawn. xterm.js
        // draws it on the next animation frame, which a shown tab's frame doesn't wait for: its
        // terminal is still paused until the IntersectionObserver reports it on screen, a frame
        // late. At fractional pixel ratios the addon's ResizeObserver also fits the canvas to the
        // device pixels on the first layout, which clears it again. Observing the canvas after
        // the addon does, this is called right after that, and the frame is painted after it:
        // drawing then, past the pause, keeps the frame from showing an empty terminal.
        // (On xterm.js master the addon draws right away after fitting the canvas, but not
        // while paused, and a new renderer still waits for the pause to end.)
        const canvas = added[added.length - 1];
        if (canvas) {
          sized = new ResizeObserver(() => {
            sized?.disconnect();
            sized = undefined;
            (term as unknown as Internals)._core?._renderService?._renderRows?.(0, term.rows - 1);
          });
          sized.observe(canvas);
        }
      } catch (e) {
        // The terminal keeps an addon whose activation failed until it is disposed.
        webgl?.dispose();
        unavailable = true;
        console.warn("WebGL renderer unavailable, using DOM renderer", e);
      }
    },
    release() {
      clearTimeout(retry);
      retry = undefined;
      sized?.disconnect();
      sized = undefined;
      // Does nothing once the terminal has disposed it (unmounting).
      addon?.dispose();
      addon = undefined;
      pages.clear();
      // The addon leaves its context to the garbage collector, which keeps the context's
      // memory (and its place among the web view's contexts) until it runs.
      gl?.getExtension("WEBGL_lose_context")?.loseContext();
      gl = undefined;
    },
  };
  renderers.add(renderer);
  renderer.load();
  return () => {
    renderers.delete(renderer);
    // After the other effects of the same commit: the terminals of a tab shown in place of
    // this one load their addons first, and share this one's atlas (by font, colors and pixel
    // ratio) rather than build it again from nothing once it is gone.
    const release = () => {
      clearTimeout(timer);
      pendingReleases.delete(term);
      renderer.release();
    };
    const timer = setTimeout(release, 0);
    pendingReleases.set(term, release);
  };
}
