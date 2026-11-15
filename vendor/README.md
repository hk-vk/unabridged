# Vendored crates

## gpui-pre-macos 0.3.12

Unmodified copy of the crates.io release (Apache-2.0, see its `LICENSE-APACHE`) except for
`src/window.rs`, where three calls to `Option::map_or_default` (unstable in Rust 1.97,
tracking issue rust-lang/rust#138099) are rewritten as `.map(..).unwrap_or_default()`.
It is wired in through `[patch.crates-io]` in the workspace `Cargo.toml`. Delete this
directory and the patch entry once the project moves to a toolchain where that API is stable.
