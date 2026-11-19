#!/bin/sh
# Prints the GitHub release notes for a tag: its CHANGELOG.md section, how to
# install, and the commits since the previous tag (none for the first).
#
# usage: tools/release-notes.sh v0.1.0 [path/to/Unabridged-0.1.0.zip]
set -eu

tag=$1
zip=${2:-}
version=${tag#v}
root=$(git rev-parse --show-toplevel)
repo_url=https://github.com/hk-vk/unabridged

notes=$(awk -v v="## $version" '$0 == v { on = 1; next } /^## / { on = 0 } on' "$root/CHANGELOG.md")
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
    echo "error: CHANGELOG.md has no section for $version" >&2
    exit 1
fi
previous=$(git describe --tags --abbrev=0 --match 'v*' "$tag^" 2>/dev/null || true)

printf '%s\n\n' "$(printf '%s\n' "$notes" | sed -e '/./,$!d')"
cat <<NOTES
## Install

\`\`\`sh
brew install --cask hk-vk/tap/unabridged
\`\`\`

Or download Unabridged-$version.zip below, unzip it and move Unabridged to Applications. It isn't notarized by Apple, so the first time you open a downloaded copy, click **Open Anyway** in System Settings → Privacy & Security.

Needs a Mac with Apple silicon and macOS 14 or later. Read along needs macOS 26.
NOTES
if [ -n "$zip" ]; then
    printf '\nSHA-256: `%s`\n' "$(shasum -a 256 "$zip" | cut -d' ' -f1)"
fi
# Commits since the previous release. The first release has none to compare
# against, and its whole history would only be noise.
if [ -n "$previous" ]; then
    printf '\n## Commits\n\n'
    git log --no-merges --reverse --format='- %s (%h)' "$previous..$tag"
    printf '\nFull list of changes: %s/compare/%s...%s\n' "$repo_url" "$previous" "$tag"
fi
