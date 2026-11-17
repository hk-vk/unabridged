#!/bin/sh
# Measure Better Audible's memory through a scripted tour (debug build).
#
# usage: tools/measure-memory.sh <label> [out-dir]
#
# Builds the app, copies it into a signed test bundle (so Keychain access
# works as in the installed app), quits the installed app, runs the tour from
# src/tour.rs and prints the footprint at each step. Silent audio is used for
# the playing steps.
set -eu

label=${1:?label}
root=$(cd "$(dirname "$0")/.." && pwd)
out=${2:-${TMPDIR:-/tmp}/better-audible-memory}
mkdir -p "$out"
bundle="$out/Better Audible.app"
identity="Better Audible Local Signing"

cargo build --quiet --manifest-path "$root/Cargo.toml" -p better-audible 2>&1 | grep -E "^error" && exit 1 || true
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$root/assets/macos/Info.plist" "$bundle/Contents/Info.plist"
cp "$root/target/debug/better-audible" "$bundle/Contents/MacOS/better-audible"
if security find-certificate -c "$identity" >/dev/null 2>&1; then
    codesign --force --sign "$identity" "$bundle" 2>/dev/null
else
    codesign --force --sign - "$bundle" 2>/dev/null
fi

audio="$out/silence.wav"
[ -f "$audio" ] || python3 -c "
import wave
with wave.open('$audio', 'wb') as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(8000)
    w.writeframes(bytes(8000 * 2 * 600))
"

installed=$(pgrep -f "Applications/Better Audible.app/Contents/MacOS/better-audible$" | head -1 || true)
if [ -n "$installed" ]; then kill -TERM "$installed"; sleep 2; fi

log="$out/$label.log"
# Launch through LaunchServices so the window comes to the front: GPUI does
# not draw a window that is fully covered, and covers load only when drawn.
: >"$log"
open -n -a "$bundle" --env BETTER_AUDIBLE_TOUR=1 --env BETTER_AUDIBLE_TOUR_AUDIO="$audio" \
    --stdout "$log" --stderr "$out/$label.err"
sleep 2
pid=$(pgrep -f "^$bundle/Contents/MacOS/better-audible$" | head -1)
results="$out/$label.tsv"
: >"$results"
seen=0
end=$(( $(date +%s) + 300 ))
while [ "$(date +%s)" -lt "$end" ] && kill -0 "$pid" 2>/dev/null; do
    count=$(grep -c '^TOUR' "$log" || true)
    while [ "$seen" -lt "$count" ]; do
        seen=$((seen + 1))
        step=$(grep '^TOUR' "$log" | sed -n "${seen}p" | cut -d' ' -f2-)
        footprint -v "$pid" >"$out/$label.$seen.footprint" 2>/dev/null || true
        mb=$(footprint "$pid" 2>/dev/null | awk '/Footprint:/ {print $(NF-5), $(NF-4); exit}')
        cpu=$(top -l 2 -s 1 -pid "$pid" -stats cpu | tail -1)
        parts=$(footprint "$pid" 2>/dev/null | awk '
            / Malloc Small$/ {m=$1" "$2} / IOSurface$/ {s=$1" "$2}
            /IOAccelerator \(graphics\)$/ {g=$1" "$2}
            END {printf "malloc %s, surfaces %s, gpu %s", m, s, g}')
        images=$(heap -s "$pid" 2>/dev/null | sed -n 's/.* 992KB\[\([0-9]*\)\].*/\1/p' | head -1)
        printf '%s\t%s\t%s%%\t%s\t1MB-images %s\n' "$step" "$mb" "$cpu" "$parts" "${images:-0}" | tee -a "$results"
        [ "$step" = "done" ] && end=0
    done
    sleep 0.5
done
peak=$(footprint "$pid" 2>/dev/null | awk '/phys_footprint_peak/ {print $2, $3}')
printf 'peak\t%s\n' "$peak" | tee -a "$results"
kill -TERM "$pid" 2>/dev/null || true
grep '^tour:' "$out/$label.err" 2>/dev/null || true
