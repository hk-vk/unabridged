#!/bin/sh
# Builds a signed, self-updating Unabridged.app into target/release-app.
#
# usage: tools/build-release.sh [signing identity]
#
# Uses "Unabridged Signing" (the identity CI signs with, made once by
# tools/release-setup.sh's steps) when no identity is given and it is in the
# keychain, otherwise signs ad hoc. The
# release bundle is named Unabridged with its own bundle identifier; the app's
# data folders are the same as the local build's, so both see one library.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
plist_tool=/usr/libexec/PlistBuddy

if [ -n "${1:-}" ]; then
    identity=$1
elif security find-certificate -c "Unabridged Signing" >/dev/null 2>&1; then
    identity="Unabridged Signing"
else
    identity=-
    echo "warning: signing ad hoc; updates will ask for permissions again" >&2
fi

public_key_file="$root/assets/macos/sparkle-public-key.txt"
if [ ! -s "$public_key_file" ]; then
    echo "error: $public_key_file is missing; run tools/release-setup.sh once" >&2
    exit 1
fi

tools/fetch-sparkle.sh

# Apple silicon only. LSMinimumSystemVersion (14.0) in Info.plist keeps older
# macOS from opening it; MACOSX_DEPLOYMENT_TARGET is not set because it also
# applies to proc-macro dylibs and broke them. The read along helper needs
# macOS 26 and says so itself on older systems.
cargo build --release -p better-audible --target-dir "$root/target"

app="$root/target/release-app/Unabridged.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"

plist="$app/Contents/Info.plist"
cp assets/macos/Info.plist "$plist"
# Sparkle orders versions by CFBundleVersion, so it must only ever grow.
build=$(git rev-list --count HEAD)
set_key() { $plist_tool -c "Set :$1 $2" "$plist" 2>/dev/null || $plist_tool -c "Add :$1 $3 $2" "$plist"; }
set_key CFBundleName Unabridged string
set_key CFBundleDisplayName Unabridged string
set_key CFBundleIdentifier io.github.hk-vk.unabridged string
set_key CFBundleVersion "$build" string
set_key NSAppleEventsUsageDescription "Unabridged reads the Amazon sign-in tab in your browser so you don’t have to copy its address. It looks for nothing else." string
set_key SUFeedURL https://github.com/hk-vk/unabridged/releases/latest/download/appcast.xml string
set_key SUPublicEDKey "$(tr -d '[:space:]' <"$public_key_file")" string
set_key SUEnableAutomaticChecks true bool
set_key SUScheduledCheckInterval 86400 integer
set_key SUVerifyUpdateBeforeExtraction true bool

cp target/release/better-audible "$app/Contents/MacOS/better-audible"
# The read along helper, compiled here so a failure stops the release instead
# of shipping without read along (crates/app/build.rs only warns).
xcrun swiftc -O -target arm64-apple-macos26.0 crates/app/src/transcribe.swift \
    -o "$app/Contents/MacOS/ba-transcribe"
cp assets/macos/AppIcon.icns assets/macos/AppIcon-light.icns "$app/Contents/Resources/"
# Sparkle's MIT license travels with the framework (see THIRD-PARTY.md).
cp target/sparkle/LICENSE "$app/Contents/Resources/Sparkle-LICENSE"

sparkle="$app/Contents/Frameworks/Sparkle.framework"
ditto target/sparkle/Sparkle.framework "$sparkle"
# The app is not sandboxed, so Sparkle's XPC services are not needed.
rm -rf "$sparkle/Versions/B/XPCServices" "$sparkle/XPCServices"
sign() { codesign --force --options runtime --timestamp=none --sign "$identity" "$@"; }
sign "$sparkle/Versions/B/Autoupdate"
sign "$sparkle/Versions/B/Updater.app"
sign "$sparkle"
sign "$app/Contents/MacOS/ba-transcribe"
sign --entitlements assets/macos/Unabridged.entitlements "$app"
codesign --verify --strict --deep "$app"
echo "Built $app ($(lipo -archs "$app/Contents/MacOS/better-audible"), build $build)"
