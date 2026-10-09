#!/usr/bin/env bash
# Sets a new version in two steps, with a pause for editing the release notes in between,
# and builds a release of it only when asked:
#
#   scripts/release.sh prepare X.Y.Z   sets the version in src-tauri/Cargo.toml (and
#                                       Cargo.lock) and adds the version's section to
#                                       CHANGELOG.md, generated with git-cliff
#   (edit CHANGELOG.md)
#   scripts/release.sh tag [git commit options...]
#                                       runs the checks, commits "chore: release vX.Y.Z"
#                                       and tags it vX.Y.Z
#   (git push origin main vX.Y.Z)
#   scripts/release.sh publish [X.Y.Z]  runs the release workflow on the pushed tag, which
#                                       builds a draft GitHub release
#
# Every version is tagged, released or not: git-cliff starts the next version's section
# after the latest tag. Pushing the tag builds nothing.
set -euo pipefail
cd "$(dirname "$0")/.."

manifest=src-tauri/Cargo.toml
changelog=CHANGELOG.md

die() {
  echo "error: $*" >&2
  exit 1
}

current_version() {
  grep -m1 '^version = ' "$manifest" | cut -d '"' -f 2
}

prepare() {
  local version=${1:-}
  [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: $0 prepare X.Y.Z (no pre-release suffix: MSI versions are numeric)"
  [[ $(git branch --show-current) == main ]] || die "not on main"
  [[ -z $(git status --porcelain) ]] || die "the working tree has uncommitted changes"
  ! git rev-parse -q --verify "refs/tags/v$version" >/dev/null || die "tag v$version already exists"

  # The package's own version is the first `version =` line.
  awk -v version="$version" '!done && /^version = "/ { $0 = "version = \"" version "\""; done = 1 } 1' \
    "$manifest" >"$manifest.tmp"
  mv "$manifest.tmp" "$manifest"
  [[ $(current_version) == "$version" ]] || die "could not set the version in $manifest"
  # Rewrites the lock file's entry for the app without updating any dependency.
  cargo metadata --format-version 1 --manifest-path "$manifest" >/dev/null

  if grep -q '^## \[[0-9]' "$changelog"; then
    # Earlier versions' sections stay as they were edited.
    pnpm dlx git-cliff --unreleased --tag "v$version" --prepend "$changelog"
  else
    pnpm dlx git-cliff --tag "v$version" -o "$changelog"
  fi

  echo
  echo "Version set to $version. Edit the $version section of $changelog, then run:"
  echo "  $0 tag"
}

tag() {
  local version
  version=$(current_version)
  ! git rev-parse -q --verify "refs/tags/v$version" >/dev/null || die "tag v$version already exists; run '$0 prepare X.Y.Z' first"
  grep -q "^## \[$version\]" "$changelog" || die "$changelog has no section for $version"
  local unexpected
  unexpected=$(git status --porcelain | cut -c4- | grep -vxE "$manifest|src-tauri/Cargo.lock|$changelog" || true)
  [[ -z $unexpected ]] || die "unexpected changes besides the version and the changelog:
$unexpected"

  pnpm build
  pnpm test
  (cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test)

  git add "$manifest" src-tauri/Cargo.lock "$changelog"
  git commit -m "chore: release v$version" "$@"
  git tag -a "v$version" -m "ZShell v$version"

  echo
  echo "Tagged v$version. Push both (this builds nothing):"
  echo "  git push origin main v$version"
  echo "To build a draft release of it, then or later:"
  echo "  $0 publish"
}

publish() {
  local version=${1:-$(current_version)}
  git ls-remote --exit-code --tags origin "refs/tags/v$version" >/dev/null ||
    die "tag v$version is not on origin; push it first: git push origin main v$version"
  gh workflow run release.yml --ref "v$version"

  echo
  echo "Started the release workflow for v$version; it creates a draft release. Follow it with:"
  echo "  gh run list --workflow release.yml"
}

case ${1:-} in
  prepare) shift; prepare "$@" ;;
  tag) shift; tag "$@" ;;
  publish) shift; publish "$@" ;;
  *) die "usage: $0 prepare X.Y.Z | $0 tag [git commit options...] | $0 publish [X.Y.Z]" ;;
esac
