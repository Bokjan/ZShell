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

interface Renderer {
  /** The atlas pages this terminal's addon has added and not yet removed. */
  pages: Set<HTMLCanvasElement>;
  drawing(): boolean;
  load(): void;
  release(): void;
}

const renderers = new Set<Renderer>();
let renewal: ReturnType<typeof setTimeout> | undefined;

/**
 * Atlases are shared by the terminals with the same font, colors and pixel ratio, and live
 * while a terminal uses them: all the terminals drawing with WebGL let theirs go before any
 * loads a new addon. Some pages may be missed (those an atlas had before a terminal joined
 * it, once the terminal that saw them is gone), so the count errs low.
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
 * renderer when WebGL is unavailable or its context is lost.
 */
export function drawWithWebgl(term: Terminal): () => void {
  let addon: WebglAddon | undefined;
  // The canvas the addon draws on, while its context is there to release.
  let canvas: HTMLCanvasElement | undefined;
  const pages = new Set<HTMLCanvasElement>();
  const renderer: Renderer = {
    pages,
    drawing: () => addon !== undefined,
    load() {
      try {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          canvas = undefined;
          renderer.release();
        });
        webgl.onAddTextureAtlasCanvas((page) => {
          pages.add(page);
          checkAtlases();
        });
        webgl.onRemoveTextureAtlasCanvas((page) => pages.delete(page));
        const before = new Set(term.element?.querySelectorAll("canvas"));
        term.loadAddon(webgl);
        canvas = [...(term.element?.querySelectorAll("canvas") ?? [])].find((c) => !before.has(c));
        addon = webgl;
      } catch (e) {
        console.warn("WebGL renderer unavailable, using DOM renderer", e);
      }
    },
    release() {
      // Does nothing once the terminal has disposed it (unmounting).
      addon?.dispose();
      addon = undefined;
      pages.clear();
      // The addon leaves its context to the garbage collector, which keeps the context's
      // memory (and its place among the web view's contexts) until it runs.
      canvas?.getContext("webgl2")?.getExtension("WEBGL_lose_context")?.loseContext();
      canvas = undefined;
    },
  };
  renderers.add(renderer);
  renderer.load();
  return () => {
    renderers.delete(renderer);
    renderer.release();
  };
}
