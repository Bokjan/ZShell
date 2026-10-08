#!/usr/bin/env node
// Writes the third-party license notices that ship with the app to
// public/third-party-licenses.txt, which Vite copies into the frontend build (embedded in
// the executable) and Settings > About shows:
//
// - Rust crates: cargo-about, with src-tauri/about.toml (accepted licenses, release
//   targets, clarifications) and the src-tauri/about.hbs template.
// - npm packages: the runtime dependencies (not devDependencies) from
//   `pnpm licenses list --prod`, with the license files each package ships.
//
// Without cargo-about (`cargo install --locked cargo-about --features cli`) the Rust part
// is left out with a note, which is fine for local builds; in CI (`CI` is set) it is an
// error, so that a release never ships incomplete notices.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "public", "third-party-licenses.txt");
const rule = "=".repeat(80);
const separator = "-".repeat(80);

function run(command, args) {
  return execFileSync(command, args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
}

// Runs the pnpm that runs this script: on Windows `pnpm` may be a .cmd shim, which
// execFile can't start without a shell.
function pnpm(args) {
  const cli = process.env.npm_execpath;
  if (cli && /\.[cm]?js$/.test(cli)) return run(process.execPath, [cli, ...args]);
  return run(cli || "pnpm", args);
}

function rustNotices() {
  const probe = spawnSync("cargo", ["about", "--version"], { cwd: root, stdio: "ignore" });
  if (probe.status !== 0) {
    const message = "cargo-about is not installed (cargo install --locked cargo-about --features cli)";
    if (process.env.CI) {
      console.error(`error: ${message}`);
      process.exit(1);
    }
    console.warn(`warning: ${message}; leaving out the Rust crates`);
    return "The notices for Rust crates were not generated in this build.\n";
  }
  return run("cargo", [
    "about",
    "generate",
    "--locked",
    "--fail",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "src-tauri/about.hbs",
  ]);
}

/** LICENSE, LICENCE-MIT, COPYING, NOTICE and the like; not LICENSE.spdx, which is metadata. */
const isLicenseFile = (name) => /^(licen[cs]e|copying|notice)/i.test(name) && !/\.spdx$/i.test(name);

function npmNotices() {
  const byLicense = JSON.parse(pnpm(["licenses", "list", "--prod", "--json"]));
  const packages = Object.values(byLicense)
    .flat()
    .flatMap((pkg) => pkg.versions.map((version, i) => ({ ...pkg, version, path: pkg.paths[i] })))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

  return packages
    .map((pkg) => {
      const files = readdirSync(pkg.path).filter(isLicenseFile).sort();
      // Leading blank lines go, the indentation of the first line stays.
      const texts = files.map((file) => readFileSync(join(pkg.path, file), "utf8").replace(/^\s*\n/, "").trimEnd());
      if (texts.length === 0) texts.push(`The package includes no license file; see ${pkg.homepage ?? "its repository"}.`);
      const homepage = pkg.homepage ? `\n${pkg.homepage}` : "";
      return `${separator}\n${pkg.name} ${pkg.version} (${pkg.license})${homepage}\n\n${texts.join("\n\n")}\n`;
    })
    .join("\n");
}

const notices = [
  "ZShell includes the third-party software listed below, each under its own license.",
  "",
  rule,
  "Rust crates",
  rule,
  rustNotices(),
  rule,
  "JavaScript packages",
  rule,
  npmNotices(),
]
  .join("\n")
  .replace(/\r\n/g, "\n");

mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, notices);
console.log(`wrote ${output} (${Math.round(Buffer.byteLength(notices) / 1024)} KB)`);
