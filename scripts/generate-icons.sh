#!/usr/bin/env bash
# Regenerates src-tauri/icons from the master SVGs in src-tauri/icons/source:
# icon.svg (full bleed) for every platform, then icon.icns again from icon-macos.svg,
# which follows Apple's grid (824 px body with a 100 px margin on a 1024 px canvas).
set -euo pipefail
cd "$(dirname "$0")/.."

icons=src-tauri/icons
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

pnpm tauri icon "$icons/source/icon.svg"
pnpm tauri icon "$icons/source/icon-macos.svg" -o "$tmp"
cp "$tmp/icon.icns" "$icons/icon.icns"
# Desktop only: drop the mobile and extra sizes the CLI always generates.
rm -rf "$icons/android" "$icons/ios" "$icons/64x64.png"
