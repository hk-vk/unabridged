#!/bin/sh
# Build Better Audible and install it as ~/Applications/Better Audible.app.
#
# usage: tools/install-app.sh [--relaunch]
#
# The bundle is signed with the "Better Audible Local Signing" identity when it
# is in the login keychain. A stable identity keeps the app's code requirement
# the same across rebuilds, so macOS remembers "Always Allow" for its Keychain
# item instead of asking again after every install. Without it the bundle is
# signed ad hoc, which macOS treats as a new app each build.
#
# Create the identity once (self-signed, local only):
#   openssl req -x509 -newkey rsa:2048 -days 3650 -nodes \
#     -keyout key.pem -out cert.pem -subj "/CN=Better Audible Local Signing" \
#     -addext "extendedKeyUsage=critical,codeSigning" \
#     -addext "keyUsage=critical,digitalSignature" \
#     -addext "basicConstraints=critical,CA:false"
#   openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -out id.p12 -passout pass:x
#   security import id.p12 -k ~/Library/Keychains/login.keychain-db -P x -T /usr/bin/codesign
#   rm key.pem cert.pem id.p12
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
app="$HOME/Applications/Better Audible.app"
binary="$app/Contents/MacOS/better-audible"
identity="Better Audible Local Signing"

cargo build --manifest-path "$root/Cargo.toml" -p better-audible

pid=$(pgrep -f "^$binary\$" | head -1 || true)
if [ -n "$pid" ] && lsof -p "$pid" 2>/dev/null |
    awk '/\.part([[:space:]]|$)|\.partial\.m4b([[:space:]]|$)/ {found=1} END {exit !found}'; then
    echo "A download is in progress; not installing." >&2
    exit 2
fi

mkdir -p "$app/Contents/MacOS"
cp "$root/assets/macos/Info.plist" "$app/Contents/Info.plist"
cp "$root/target/debug/better-audible" "$binary.new"
mv -f "$binary.new" "$binary"

if security find-certificate -c "$identity" >/dev/null 2>&1; then
    codesign --force --deep --sign "$identity" "$app"
else
    echo "warning: \"$identity\" not found; signing ad hoc (Keychain will ask again)." >&2
    codesign --force --deep --sign - "$app"
fi
codesign --verify "$app"
echo "Installed $app"

if [ "${1:-}" = "--relaunch" ]; then
    if [ -n "$pid" ]; then
        kill -TERM "$pid"
        i=0
        while kill -0 "$pid" 2>/dev/null && [ $i -lt 20 ]; do sleep 0.25; i=$((i + 1)); done
    fi
    open "$app"
fi
