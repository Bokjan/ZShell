#!/usr/bin/env node
// Writes the third-party license notices that ship with the app to
// public/third-party-licenses.json, which Vite copies into the frontend build (embedded in
// the executable, brotli-compressed by Tauri) and Settings > About shows:
//
// - Rust crates: cargo-about's JSON output, with src-tauri/about.toml (accepted licenses,
//   release targets, clarifications).
// - npm packages: the runtime dependencies (not devDependencies) from
//   `pnpm licenses list --prod`, with the license files each package ships.
//
// The output lists every package with the indices of its license texts; identical texts
// are stored once. A package whose license isn't in about.toml's `accepted` list fails the
// generation, npm packages as well as crates, so that a new dependency's license is
// checked before it ships.
//
// Without cargo-about (`cargo install --locked cargo-about --features cli`) the Rust part
// is left out and `rustMissing` is set, which is fine for local builds; in CI (`CI` is
// set) it is an error, so that a release never ships incomplete notices.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(root, "public", "third-party-licenses.json");

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

const texts = [];
const textIndex = new Map();
/** The index of a license text, adding it if it's new. */
function addText(text) {
  // Leading blank lines go, the indentation of the first line stays.
  const normalized = text.replace(/\r\n/g, "\n").replace(/^\s*\n/, "").trimEnd();
  let index = textIndex.get(normalized);
  if (index === undefined) {
    index = texts.push(normalized) - 1;
    textIndex.set(normalized, index);
  }
  return index;
}

function rustPackages() {
  const probe = spawnSync("cargo", ["about", "--version"], { cwd: root, stdio: "ignore" });
  if (probe.status !== 0) {
    const message = "cargo-about is not installed (cargo install --locked cargo-about --features cli)";
    if (process.env.CI) {
      console.error(`error: ${message}`);
      process.exit(1);
    }
    console.warn(`warning: ${message}; leaving out the Rust crates`);
    return null;
  }
  // Into a file: on Windows cargo-about refuses to write to stdout when it detects
  // PowerShell, which CI runners use.
  const dir = mkdtempSync(join(tmpdir(), "zshell-about-"));
  const file = join(dir, "about.json");
  let about;
  try {
    run("cargo", [
      "about",
      "generate",
      "--locked",
      "--fail",
      "--format",
      "json",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "-c",
      "src-tauri/about.toml",
      "-o",
      file,
    ]);
    about = JSON.parse(readFileSync(file, "utf8"));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  // cargo-about lists licenses with the crates using them; turn that around.
  const crates = new Map();
  for (const license of about.licenses) {
    const index = addText(license.text);
    for (const { crate } of license.used_by) {
      const key = `${crate.name}@${crate.version}`;
      if (!crates.has(key)) {
        crates.set(key, {
          name: crate.name,
          version: crate.version,
          license: crate.license ?? license.id,
          url: crate.repository ?? crate.homepage ?? null,
          texts: [],
        });
      }
      const texts = crates.get(key).texts;
      if (!texts.includes(index)) texts.push(index);
    }
  }
  return [...crates.values()];
}

/** The licenses we can comply with: about.toml's `accepted` list, which cargo-about enforces for crates. */
const accepted = new Set(
  [...readFileSync(join(root, "src-tauri", "about.toml"), "utf8").match(/^accepted = \[([^\]]*)\]/m)[1].matchAll(/"([^"]+)"/g)].map(
    (match) => match[1],
  ),
);

/** Whether an SPDX expression ("MIT", "(MIT OR Apache-2.0) AND BSD-3-Clause") can be met with accepted licenses. */
function isAccepted(expression) {
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? [];
  let i = 0;
  // OR binds looser than AND, as in SPDX.
  const or = () => {
    let ok = and();
    while (tokens[i] === "OR") {
      i++;
      ok = and() || ok;
    }
    return ok;
  };
  const and = () => {
    let ok = term();
    while (tokens[i] === "AND") {
      i++;
      ok = term() && ok;
    }
    return ok;
  };
  const term = () => {
    if (tokens[i] === "(") {
      i++;
      const ok = or();
      i++;
      return ok;
    }
    return accepted.has(tokens[i++]);
  };
  return or() && i === tokens.length;
}

/** LICENSE, LICENCE-MIT, COPYING, NOTICE and the like; not LICENSE.spdx, which is metadata. */
const isLicenseFile = (name) => /^(licen[cs]e|copying|notice)/i.test(name) && !/\.spdx$/i.test(name);

function npmPackages() {
  const byLicense = JSON.parse(pnpm(["licenses", "list", "--prod", "--json"]));
  return Object.values(byLicense)
    .flat()
    .flatMap((pkg) => pkg.versions.map((version, i) => ({ ...pkg, version, path: pkg.paths[i] })))
    .map((pkg) => {
      const files = readdirSync(pkg.path).filter(isLicenseFile).sort();
      const contents = files.map((file) => readFileSync(join(pkg.path, file), "utf8"));
      if (contents.length === 0) {
        contents.push(`The package includes no license file; see ${pkg.homepage ?? "its repository"}.`);
      }
      return {
        name: pkg.name,
        version: pkg.version,
        license: pkg.license,
        url: pkg.homepage ?? null,
        texts: contents.map(addText),
      };
    });
}

const npm = npmPackages();
const refused = npm.filter((pkg) => !isAccepted(pkg.license ?? ""));
if (refused.length > 0) {
  for (const pkg of refused) {
    console.error(`error: ${pkg.name}@${pkg.version} has a license not in src-tauri/about.toml's accepted list: ${pkg.license}`);
  }
  process.exit(1);
}

const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
const rust = rustPackages();
const notices = {
  rust: (rust ?? []).sort(byName),
  rustMissing: rust === null,
  npm: npm.sort(byName),
  texts,
};

const json = JSON.stringify(notices);
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, json);
console.log(
  `wrote ${output} (${notices.rust.length} crates, ${notices.npm.length} npm packages, ` +
    `${texts.length} texts, ${Math.round(Buffer.byteLength(json) / 1024)} KB)`,
);
