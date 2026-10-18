/**
 * The elements a pane's terminal and side panels are drawn into (through portals, see
 * `PaneHost`), the same for as long as the pane is open. The tab the pane is in puts them in
 * its page; moving the pane to another tab moves the elements, and nothing remounts: React
 * would unmount a terminal whose parent changes, which ends its session.
 *
 * Neither element makes a box of its own (`display: contents`), so the page lays out their
 * content as if it were its own.
 */
export interface PaneHosts {
  /** Where pane `key`'s terminal is drawn; made when first asked for. */
  terminal(key: number): HTMLElement;
  /** Where pane `key`'s side panels are drawn; made when first asked for. */
  panels(key: number): HTMLElement;
  /** Forgets the elements of panes other than `keys`, which have closed or moved away. */
  retain(keys: ReadonlySet<number>): void;
}

export function createPaneHosts(): PaneHosts {
  const hosts = new Map<number, { terminal: HTMLElement; panels: HTMLElement }>();
  const element = (className: string) => {
    const div = document.createElement("div");
    div.className = className;
    return div;
  };
  const of = (key: number) => {
    let host = hosts.get(key);
    if (!host) {
      host = { terminal: element("pane-host"), panels: element("pane-host") };
      hosts.set(key, host);
    }
    return host;
  };
  return {
    terminal: (key) => of(key).terminal,
    panels: (key) => of(key).panels,
    retain(keys) {
      for (const key of hosts.keys()) if (!keys.has(key)) hosts.delete(key);
    },
  };
}

/**
 * Puts `hosts` into `container` (first, before what the container has of its own) while it
 * holds them, and takes out those still there when done: a host that has moved on to another
 * container meanwhile is left where it is.
 */
export function place(container: HTMLElement, hosts: HTMLElement[]): () => void {
  container.prepend(...hosts);
  return () => hosts.forEach((host) => host.parentNode === container && host.remove());
}
