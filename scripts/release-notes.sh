#!/usr/bin/env bash
# Prints the GitHub release notes for version X.Y.Z, taken from CHANGELOG.md:
#
#   scripts/release-notes.sh X.Y.Z
#
# Not every tagged version is released, so the notes cover every version since the latest
# published GitHub release before X.Y.Z (drafts don't count): just X.Y.Z's section when
# that release is the version before it, or else each version's section under its own
# heading, newest first. Used by the release workflow; needs `gh` signed in (GH_TOKEN in CI).
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
  echo "error: $*" >&2
  exit 1
}

version=${1:-}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "usage: $0 X.Y.Z"
grep -q "^## \[$version\]" CHANGELOG.md || die "CHANGELOG.md has no section for $version"

# The published releases' versions with this one, in order; the one before it is the
# previous release (none when this is the first).
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
previous=$(
  {
    gh api --paginate "repos/$repo/releases" --jq '.[] | select(.draft | not) | .tag_name' |
      sed -n 's/^v\([0-9]*\.[0-9]*\.[0-9]*\)$/\1/p'
    echo "$version"
  } | sort -uV | awk -v version="$version" '$0 == version { print previous; exit } { previous = $0 }'
)

# CHANGELOG.md lists the versions newest first: from X.Y.Z's heading up to the previous
# release's (or the end). A single version keeps its section as is; several get their
# version and date as headings, without the brackets.
awk -v version="$version" -v previous="$previous" '
  /^## \[/ {
    v = substr($0, 5, index($0, "]") - 5)
    if (v == version) found = 1
    if (v == previous) exit
    if (found) { n++; headings[n] = "## " v substr($0, index($0, "]") + 1); next }
  }
  found { lines[n] = lines[n] $0 "\n" }
  END {
    for (i = 1; i <= n; i++) {
      if (n > 1) printf "%s\n\n", headings[i]
      # Without the blank lines around each section.
      body = lines[i]
      sub(/^\n+/, "", body)
      sub(/\n+$/, "", body)
      printf "%s\n", body
      if (i < n) printf "\n"
    }
  }' CHANGELOG.md
