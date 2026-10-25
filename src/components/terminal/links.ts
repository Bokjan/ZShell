import type { Terminal } from "@xterm/xterm";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Disposables, type Disposable } from "../../lib/disposables";
import { isMac } from "../../lib/platform";
import { TOOLTIP_DELAY } from "../Tooltip";

/**
 * How long a link's tooltip stays after the pointer leaves the link: xterm.js leaves and hovers
 * it again whenever its line is drawn again (the cursor blinking on it, output).
 */
const LINK_LEAVE_MS = 100;

/** The tooltip of the link under the pointer; `uri` for links whose text may not say where they go. */
export interface LinkHint {
  anchor: Pick<Element, "getBoundingClientRect">;
  uri: string | null;
}

/** The terminal's links; `current` is the one under the pointer, if any. */
export interface Links extends Disposable {
  current(): string | null;
}

/**
 * Links open with ⌘-click on macOS and Ctrl+click elsewhere, as in other terminals. xterm.js
 * activates a link on any button's release: with the right one, the context menu opens.
 */
const opensLink = (e: MouseEvent) => e.button === 0 && (isMac ? e.metaKey : e.ctrlKey);

/**
 * Links: addresses in the text, and those programs mark (OSC 8: `ls --hyperlink`, gcc), whose
 * text may be anything, so their tooltip (`showHint`) shows where they go. A plain click only
 * selects or focuses, as on any other text. xterm.js only gives http and https ones. Before
 * the terminal is opened in `container`.
 */
export function attachLinks(term: Terminal, container: HTMLElement, showHint: (hint: LinkHint | null) => void): Links {
  const held = new Disposables();
  const timer = held.timer();
  let link: string | null = null;
  // The link whose tooltip is shown, and the one whose tooltip was dismissed (until left).
  let hinted: string | null = null;
  let dismissed: string | null = null;
  const activate = (e: MouseEvent, uri: string) => {
    if (opensLink(e)) void openUrl(uri).catch(console.error);
  };
  const hover = (e: MouseEvent, uri: string, marked: boolean) => {
    timer.clear();
    link = uri;
    if (uri === hinted || uri === dismissed) return;
    // Below the line the pointer is on.
    const screen = term.element?.querySelector(".xterm-screen")?.getBoundingClientRect() ?? container.getBoundingClientRect();
    const height = screen.height / term.rows;
    const top = screen.top + Math.floor((e.clientY - screen.top) / height) * height;
    const rect = new DOMRect(e.clientX, top, 0, height);
    const hint = { anchor: { getBoundingClientRect: () => rect }, uri: marked ? uri : null };
    timer.start(
      () => {
        hinted = uri;
        showHint(hint);
      },
      hinted === null ? TOOLTIP_DELAY : 0,
    );
  };
  const leave = () => {
    link = null;
    timer.start(() => {
      hinted = dismissed = null;
      showHint(null);
    }, LINK_LEAVE_MS);
  };
  // Clicking or scrolling dismisses it, as other tooltips are.
  const dismissHint = () => {
    if (!link) return;
    timer.clear();
    dismissed = link;
    hinted = null;
    showHint(null);
  };
  term.options.linkHandler = { activate, hover: (e, uri) => hover(e, uri, true), leave };
  term.loadAddon(new WebLinksAddon(activate, { hover: (e, uri) => hover(e, uri, false), leave }));
  held.listen(container, "mousedown", dismissHint, true);
  held.listen(container, "wheel", dismissHint, true);
  return { current: () => link, dispose: () => held.dispose() };
}
