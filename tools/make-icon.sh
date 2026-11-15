#!/bin/sh
# Build assets/macos/AppIcon.icns from a 1024x1024 PNG of the finished icon
# (squircle tile on a transparent canvas, per Apple's icon grid).
#
# usage: tools/make-icon.sh [source.png]   (default: assets/macos/AppIcon.png)
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
src=${1:-"$root/assets/macos/AppIcon.png"}
set_dir=$(mktemp -d)/AppIcon.iconset
mkdir -p "$set_dir"

for size in 16 32 128 256 512; do
    sips -z $size $size "$src" --out "$set_dir/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$src" --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$root/assets/macos/AppIcon.icns"
rm -rf "$(dirname "$set_dir")"
echo "Wrote $root/assets/macos/AppIcon.icns"
