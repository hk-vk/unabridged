#!/bin/sh
# One-time setup for publishing releases. Safe to run again.
#
# 1. Creates (or reuses) the Sparkle update-signing key in your login keychain,
#    under its own account so it never mixes with another app's key, and writes
#    the public half to assets/macos/sparkle-public-key.txt (commit that file).
# 2. Prints the GitHub secrets the release workflow needs.
#
# usage: tools/release-setup.sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
account=unabridged
"$root/tools/fetch-sparkle.sh"
keys="$root/target/sparkle/bin/generate_keys"

"$keys" --account "$account" >/dev/null
"$keys" --account "$account" -p | tr -d '[:space:]' >"$root/assets/macos/sparkle-public-key.txt"
echo "Public update key: $(cat "$root/assets/macos/sparkle-public-key.txt")"
echo "Written to assets/macos/sparkle-public-key.txt; commit it."

cat <<'STEPS'

Then add these repository secrets (Settings > Secrets and variables > Actions),
or with the gh commands shown:

  SPARKLE_PRIVATE_KEY   the update-signing private key
      target/sparkle/bin/generate_keys --account unabridged -x key.txt
      gh secret set SPARKLE_PRIVATE_KEY -R hk-vk/unabridged < key.txt && rm key.txt

  SIGNING_P12, SIGNING_P12_PASSWORD   the "Unabridged Signing" identity
      A self-signed code-signing certificate (made with openssl, like
      BadgeBar's create-signing-cert.sh), imported into the login keychain
      for local release builds and uploaded as a base64 .p12 with its password.

  TAP_DEPLOY_KEY   an SSH key added as a write deploy key on hk-vk/homebrew-tap
      ("unabridged release workflow")

Keep a copy of the private update key somewhere safe: without it, installed
copies can never be updated again.
STEPS
