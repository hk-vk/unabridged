# Unabridged

**A better Audible client for the Mac.**

On a Mac, Audible's app is its iPhone and iPad app, installed from the App
Store. It works, but it doesn't feel like a Mac app. Unabridged is a native,
fast, good-looking Mac app for the Audible library you already own. Sign in
with your Audible account and you get your whole library,
offline downloads, a player built around chapters, a read-along transcript and
a floating mini player, with your place kept in sync with your phone.

> Unabridged is an independent project. It is not made, endorsed or supported by
> Audible or Amazon. "Audible" is a trademark of Audible, Inc.

## Why Unabridged

- **It's a real Mac app.** Native windows, keyboard shortcuts, light and dark
  mode, a mini player that floats above your work. Not an iPad app in a window.
- **It looks like your books.** Every book's player takes its colour from the
  cover, and progress shows right on the covers.
- **It reads along with you.** An on-device transcript follows the narration
  line by line, and you can click any line to jump there.
- **It works offline.** Download books to your Mac and listen anywhere.
- **It stays in sync.** Your position syncs with Audible, so you can switch
  between your Mac and your phone mid-chapter.
- **It's free and open source,** with no tracking.

## Features

- **Your library:** Library, In progress, On this Mac, Bookmarks, Authors,
  Series and Finished, with search across titles, authors and narrators.
- **Progress you can see:** progress bars on covers, chapter-by-chapter progress
  for the book you're playing, and "Almost done" when a book is nearly finished.
- **Now Playing:** a chapter-first player with a 30-second skip, speeds from
  0.75× to 2.5× in fine steps, a sleep timer (minutes or end of chapter),
  bookmarks and a chapter list.
- **Mini player:** a small floating window that stays above other apps.
- **Read along:** an on-device transcript that follows the narration. Click a
  line to jump there. The text size can be changed (⌘+ / ⌘−).
- **Sync:** your listening position syncs with Audible, so you can switch
  between your Mac and your phone.
- **Explore:** browse Audible's catalog and new releases, then buy on Audible's
  website.
- **Appearance:** light, dark or automatic; five accent colours; a light or dark
  app icon.
- **Updates:** automatic, signed updates.

## Requirements

- A Mac with Apple silicon (M1 or later). Intel Macs are not supported.
- macOS 14 Sonoma or later.
- Read along needs macOS 26 or later, because it uses Apple's on-device speech
  recognition.
- An Audible account in one of these stores: United States, United Kingdom,
  Germany, France, Canada, Italy, Australia, India, Japan, Spain or Brazil.

## Install

With [Homebrew](https://brew.sh):

```sh
brew install --cask hk-vk/tap/unabridged
```

Or download the latest `Unabridged-<version>.zip` from
[Releases](https://github.com/hk-vk/unabridged/releases), unzip it and move
**Unabridged** to your Applications folder.

The app is signed but not notarized by Apple, so the first time you open a
downloaded copy macOS will block it (Homebrew installs skip this). To allow it:

1. Try to open Unabridged once, then close the warning.
2. Open **System Settings → Privacy & Security**.
3. Scroll down and click **Open Anyway** next to the Unabridged message.

### Updates

Unabridged checks for updates once a day and asks before installing one. You can
also check from **Settings → About → Check for Updates…**. Updates are signed and
verified before they're installed.

## Signing in

Unabridged signs in through your own web browser, not a window inside the app:

1. Choose your Audible store.
2. Sign in on Amazon's page in your browser.
3. When Amazon shows a "page not found" address, copy it. Unabridged notices the
   copied address and finishes signing in.

This registers your Mac as a device on your Audible account, the same way the
Audible iPhone app does. You can remove it later from your Audible account's
device list.

## Privacy

- Unabridged talks only to Amazon and Audible, to sign in, load your library,
  sync your position and download your books.
- There are no analytics, no tracking and no servers of ours.
- Your sign-in is stored in private files in
  `~/Library/Application Support/Better Audible/`, readable only by your user
  account.
- Read along transcribes on your Mac. No audio or text leaves it.

## Downloads and DRM

Audible books are protected. To play a book offline, Unabridged downloads it with
your account, unlocks it with the key Audible issues to your registered device,
and saves a playable copy on your Mac. This is the same approach used by
open-source tools such as [Libation](https://github.com/rmcrackan/Libation) and
[audible-cli](https://github.com/mkb79/audible-cli).

Use it only for books you have bought, on your own devices. Don't share the
files. A few newer titles use a protection scheme Unabridged can't unlock; for
those the download stops with an error explaining why.

## Building from source

You need:

- Rust 1.90 or later (`rustup`)
- Xcode 26 or later, for the read along helper (it builds against the macOS 26
  SDK)

```sh
git clone https://github.com/hk-vk/unabridged.git
cd unabridged
cargo run -p better-audible          # run a debug build
tools/install-app.sh                 # build and install ~/Applications/Better Audible.app
```

Run the tests with `cargo test --workspace`.

A local build is called "Better Audible" and doesn't update itself. Release
builds come from `tools/build-release.sh`; see
[docs/releasing.md](docs/releasing.md).

### Project layout

| Path | What it holds |
| --- | --- |
| `crates/app` | The Mac app: windows, screens, playback, read along |
| `crates/ui` | Design tokens, type scale and components |
| `crates/audible` | Audible sign-in, library and catalog API, downloads, decryption ([notes](crates/audible/README.md)) |
| `crates/model` | Shared book types and text formatting |
| `assets` | Fonts, icons and the app bundle files |
| `vendor` | A patched copy of `gpui-pre-macos` ([why](vendor/README.md)) |
| `tools` | Install, icon and measurement scripts |

## Credits

- Built with [GPUI](https://github.com/zed-industries/zed), the UI framework
  from Zed.
- Sign-in and download protocol work follows
  [mkb79/audible](https://github.com/mkb79/Audible) and
  [Libation](https://github.com/rmcrackan/Libation).
- Type: [Allo](https://allo.joshpuckett.me) by Josh Puckett,
  [Literata](https://github.com/googlefonts/literata) and Geist Pixel, all under
  the SIL Open Font License (`assets/fonts`).
- Icons: [Hugeicons](https://hugeicons.com) free set, MIT (`assets/icons`).

## License

Unabridged is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Bundled fonts, icons and the vendored GPUI crate keep their own
licenses, included next to them.
