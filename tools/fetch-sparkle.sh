#!/bin/sh
# Downloads Sparkle into target/sparkle (framework plus generate_keys and
# generate_appcast). Release builds embed the framework; nothing else needs it.
#
# usage: tools/fetch-sparkle.sh
set -eu

version=2.10.0
root=$(cd "$(dirname "$0")/.." && pwd)
dir="$root/target/sparkle"
if [ -d "$dir/Sparkle.framework" ] && [ -x "$dir/bin/generate_appcast" ]; then
    exit 0
fi
mkdir -p "$dir"
curl -sSfL -o "$dir/sparkle.tar.xz" \
    "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz"
tar -xf "$dir/sparkle.tar.xz" -C "$dir"
rm "$dir/sparkle.tar.xz"
echo "Sparkle $version in $dir"
