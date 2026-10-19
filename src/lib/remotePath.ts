/** Paths on the server, which are always `/`-separated. */

/** `name` in folder `dir`. */
export const joinPath = (dir: string, name: string) => (dir.endsWith("/") ? dir + name : `${dir}/${name}`);

/** The folder `path` is in; `/` for what is in the root, and for the root itself. */
export const parentPath = (path: string) => path.replace(/\/[^/]+\/?$/, "") || "/";
