//! Better Audible design system.
//!
//! - [`tokens`] holds the raw design values; [`theme`] resolves them into roles
//!   for the current mode and accent, plus cover-derived Now Playing palettes.
//! - [`typography`] defines the type scale and registers the bundled fonts.
//! - [`metrics`] holds shape and size tokens.
//!
//! Call [`init`] once at startup.

pub mod color;
pub mod metrics;
pub mod theme;
pub mod tokens;
pub mod typography;

pub use theme::{ActiveTheme, Theme, ThemeSettings};
pub use typography::{TypeStyle, TypographyExt, text};

use gpui_kit::App;

/// Registers fonts and installs the theme. Call once, before opening windows.
pub fn init(settings: ThemeSettings, cx: &mut App) -> anyhow::Result<()> {
    typography::register_fonts(cx)?;
    Theme::init(settings, cx);
    Ok(())
}
