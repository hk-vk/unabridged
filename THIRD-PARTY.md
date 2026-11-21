# Third-party software

Unabridged is MIT OR Apache-2.0 (see `LICENSE-MIT` and `LICENSE-APACHE`). It
includes or builds on the following, each under its own license.

## Shipped in the app

| Component | License | Where |
| --- | --- | --- |
| [Sparkle](https://sparkle-project.org) 2.10.0, the update framework | MIT | Embedded in release builds as `Sparkle.framework`; its license is copied to `Contents/Resources/Sparkle-LICENSE` |
| [gpui-pre-macos](https://crates.io/crates/gpui-pre-macos) 0.3.12, from [Zed](https://github.com/zed-industries/zed) | Apache-2.0 | `vendor/gpui-pre-macos`, with a three-line patch described in `vendor/README.md` |
| [Allo](https://allo.joshpuckett.me) by Josh Puckett | SIL OFL 1.1 | `assets/fonts/allo` |
| [Literata](https://github.com/googlefonts/literata) | SIL OFL 1.1 | `assets/fonts/literata` |
| Geist Pixel | SIL OFL 1.1 | `assets/fonts/geist-pixel` |
| [Hugeicons](https://hugeicons.com) free set | MIT | `assets/icons` |
| Rust crates listed in `Cargo.lock` | Their own licenses, mostly MIT and/or Apache-2.0 | Compiled into the app |
| [whisper-rs](https://codeberg.org/tazz4843/whisper-rs) / [whisper.cpp](https://github.com/ggml-org/whisper.cpp) | Unlicense / MIT | Local speech inference |
| [Symphonia](https://github.com/pdeljanov/Symphonia) | MPL-2.0 | Audiobook decoding |

## Optional speech model

Read along can download the multilingual Whisper base model, quantised as
[`ggml-base-q8_0.bin`](https://huggingface.co/ggerganov/whisper.cpp/blob/main/ggml-base-q8_0.bin)
(81,768,585 bytes), converted from [OpenAI Whisper](https://github.com/openai/whisper)
(MIT). SHA-256: `c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9`.
The model is downloaded only on request and can be removed in Settings.

## Code derived from other projects

- **AAX key derivation** (`crates/audible/src/drm/aax.rs`) follows
  `mov_read_adrm` in [FFmpeg](https://ffmpeg.org)'s `libavformat/mov.c`
  (LGPL-2.1-or-later), as does AAXClean. The fixed key it uses was published
  by the FFmpeg developers.

## Protocol references

These were read to understand Audible's sign-in, licensing and download
protocol. No code from them is included; where a value such as a device
profile or user agent comes from them, the source comment says so.

- [mkb79/audible](https://github.com/mkb79/Audible) and
  [audible-cli](https://github.com/mkb79/audible-cli) (AGPL-3.0)
- [Libation](https://github.com/rmcrackan/Libation) and AAXClean (GPL-3.0)
