import { describe, expect, it } from "vitest";

// Tauri's CLI refuses to run (`pnpm tauri dev`, the release builds) when an npm package of
// Tauri's and its crate differ in major or minor version, which CI doesn't run into otherwise:
// Dependabot updates the two sides in pull requests of their own.
const pnpmLock = Object.values(import.meta.glob<string>("../pnpm-lock.yaml", { query: "?raw", import: "default", eager: true }))[0];
const cargoLock = Object.values(import.meta.glob<string>("../src-tauri/Cargo.lock", { query: "?raw", import: "default", eager: true }))[0];

/** The npm packages of Tauri's that have a crate, with their versions: `api` (`tauri`) and the plugins. */
const npmVersions = () =>
  new Map(
    [...pnpmLock.matchAll(/^ {2}'@tauri-apps\/(api|plugin-[a-z-]+)@(\d+\.\d+\.\d+)':$/gm)].map(([, name, version]) => [name, version]),
  );

/** The versions of crate `name` in Cargo.lock. */
const crateVersions = (name: string) =>
  [...cargoLock.matchAll(/^name = "([^"]+)"\nversion = "([^"]+)"$/gm)]
    .filter(([, found]) => found === name)
    .map(([, , version]) => version);

const majorMinor = (version: string) => version.split(".").slice(0, 2).join(".");

describe("Tauri's packages", () => {
  it("are found", () => {
    expect(npmVersions().get("api")).toBeDefined();
    expect(crateVersions("tauri")).toHaveLength(1);
  });

  it("have the major and minor version of their crates", () => {
    const mismatched = [...npmVersions()].flatMap(([name, version]) => {
      const crate = name === "api" ? "tauri" : `tauri-${name}`;
      const crates = crateVersions(crate);
      return crates.some((other) => majorMinor(other) === majorMinor(version))
        ? []
        : [`@tauri-apps/${name} ${version}, ${crate} ${crates.join(", ")}`];
    });
    expect(mismatched).toEqual([]);
  });
});
