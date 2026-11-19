# Releasing Unabridged

Releases work like BadgeBar's: pushing a `v*` tag runs
`.github/workflows/release.yml`, which builds and signs `Unabridged.app`,
publishes a GitHub release with the zip and a signed `appcast.xml`, and updates
the Homebrew cask in `hk-vk/homebrew-tap`. Installed copies read the feed from
`https://github.com/hk-vk/unabridged/releases/latest/download/appcast.xml`, so
there is no website to deploy.

The app is signed with the self-signed "Unabridged Signing" identity (in the
login keychain and in the `SIGNING_P12` secret) and is not notarized. Homebrew clears the quarantine flag; people who download
the zip click Open Anyway once.

## One-time setup

1. **Update-signing key.** Run `tools/release-setup.sh`. It downloads Sparkle,
   creates an Ed25519 key in your login keychain (account `unabridged`) and
   writes the public half to `assets/macos/sparkle-public-key.txt`. Commit that
   file. Back up the private key: without it, installed copies can never update
   again.
2. **Repository secrets.** `SPARKLE_PRIVATE_KEY`, `SIGNING_P12`,
   `SIGNING_P12_PASSWORD` and `TAP_DEPLOY_KEY` (a write deploy key on
   hk-vk/homebrew-tap named "unabridged release workflow"). All four are set.
3. **Test a local release build.** Run `tools/build-release.sh`, open
   `target/release-app/Unabridged.app` (quit the local build first, since they
   share the same sign-in and library), check that Settings → About shows
   Check for Updates…, and quit it.
4. **Dry run in CI.** Run the Release workflow by hand with the next version's
   tag, unpushed (`gh workflow run release.yml -R hk-vk/unabridged -f
   tag=v0.1.0`). It tags the run's commit itself, uploads the zip, notes and
   appcast as an artifact and publishes nothing.
5. **Homebrew cask.** `Casks/unabridged.rb` must exist in hk-vk/homebrew-tap
   before the first release; the workflow only updates its version and
   checksum, as it does for BadgeBar.
6. **Make the repo public** when you're ready (`gh repo edit hk-vk/unabridged
   --visibility public --accept-visibility-change-consequences`). The update
   feed and Homebrew downloads only work once it is public.

## Each release

1. Bump the version in `Cargo.toml` (`[workspace.package] version`) and
   `CFBundleShortVersionString` in `assets/macos/Info.plist`. The workflow
   refuses to run if they don't match the tag.
2. Add a `## <version>` section to `CHANGELOG.md`. It becomes the release notes.
3. Commit, then tag and push:

   ```sh
   git tag v0.1.0 && git push origin main v0.1.0
   ```

`CFBundleVersion` is set from the commit count at build time, so Sparkle always
sees a newer build.

## Before the first public release

- **Screenshots.** Add a few to the README (Home, Now Playing with read along,
  the mini player).
- **Trademark check.** Search "Unabridged" for software in the USPTO and EUIPO
  databases.
- **Read along on older macOS.** Dim the Read along button below macOS 26 and
  tell a helper crash apart from an old system (see the plan in the session
  notes); the README already says read along needs macOS 26.
- **Rename leftovers.** The release bundle is called Unabridged with the
  identifier `io.github.hk-vk.unabridged`. Still named Better Audible, and
  safe to leave or rename later with a migration:
  - the data folders `~/Library/Application Support/Better Audible` and
    `~/Library/Caches/Better Audible` (renaming them needs code that moves an
    existing sign-in and downloads);
  - the crate and binary `better-audible`, and the local build from
    `tools/install-app.sh`;
  - the local build's signing identity "Better Audible Local Signing" (release
    builds use "Unabridged Signing").
