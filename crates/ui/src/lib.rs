//! Better Audible design system.
//!
//! - [`tokens`] holds the raw design values; [`theme`] resolves them into roles
//!   for the current mode and accent, plus cover-derived Now Playing palettes.
//! - [`typography`] defines the type scale and registers the bundled fonts.
//! - [`metrics`] holds shape and size tokens.
//! - [`elevation`] turns named levels into clean, pixel-snapped surfaces.
//! - [`icon`] and [`assets`] bundle the Hugeicons set behind the typed [`Icon`].
//! - [`components`] holds the reusable controls.
//!
//! Call [`init`] once at startup.

pub mod assets;
pub mod color;
pub mod components;
pub mod elevation;
pub mod icon;
pub mod metrics;
pub mod theme;
pub mod tokens;
pub mod typography;

pub use assets::Assets;
pub use elevation::{Elevation, ElevationExt, Surface};
pub use icon::{Icon, icon};
pub use theme::{ActiveTheme, Theme, ThemeSettings};
pub use typography::{TypeStyle, TypographyExt, text};

use gpui_kit::App;

/// Registers fonts and installs the theme. Call once, before opening windows.
pub fn init(settings: ThemeSettings, cx: &mut App) -> anyhow::Result<()> {
    typography::register_fonts(cx)?;
    Theme::init(settings, cx);
    Ok(())
}
