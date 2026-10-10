#!/usr/bin/env bash
# Regenerates src-tauri/icons from the master SVGs in src-tauri/icons/source:
# icon.svg (full bleed) for every platform, then icon.icns again from icon-macos.svg,
# which follows Apple's grid (824 px body with a 100 px margin on a 1024 px canvas), and
# the Microsoft Store package's images in src-tauri/icons/msix.
set -euo pipefail
cd "$(dirname "$0")/.."

icons=src-tauri/icons
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

pnpm tauri icon "$icons/source/icon.svg"
pnpm tauri icon "$icons/source/icon-macos.svg" -o "$tmp/macos"
cp "$tmp/macos/icon.icns" "$icons/icon.icns"
# Only what bundle.icon in tauri.conf.json lists is used; the CLI also makes mobile icons,
# the 512 px icon.png, and Store images at a single scale, replaced by the ones below.
rm -rf "$icons/android" "$icons/ios" "$icons/64x64.png" "$icons/icon.png" "$icons"/Square*Logo.png "$icons/StoreLogo.png"

# The Store package's images, each at the scales Windows picks from (100, 200 and 400 %),
# and the app list and taskbar icon also at the exact sizes the shell draws it, without the
# plate drawn behind smaller images. The manifest names them without these qualifiers;
# scripts/package-msix.ps1 indexes them in resources.pri.
pnpm tauri icon "$icons/source/icon.svg" -o "$tmp/msix" -p 16,24,32,44,48,50,88,100,150,176,200,256,300,600
msix=$icons/msix
rm -rf "$msix"
mkdir "$msix"
image() { cp "$tmp/msix/$1x$1.png" "$msix/$2.png"; }
for scale in 100 200 400; do
  image $((44 * scale / 100)) "Square44x44Logo.scale-$scale"
  image $((150 * scale / 100)) "Square150x150Logo.scale-$scale"
  image $((50 * scale / 100)) "StoreLogo.scale-$scale"
done
for size in 16 24 32 48 256; do
  image "$size" "Square44x44Logo.targetsize-$size"
  image "$size" "Square44x44Logo.targetsize-${size}_altform-unplated"
done
