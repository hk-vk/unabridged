//! Semantic theme: resolves [`crate::tokens`] into roles for the current mode and accent.
//!
//! The resolved [`Theme`] lives in a GPUI global. Read it with
//! [`ActiveTheme::theme`]; change it with [`Theme::apply_settings`] or follow the
//! system with [`Theme::sync_system_appearance`].

mod accent;
mod book;

pub use accent::{Accent, AccentTheme};
pub use book::BookPalette;

use gpui_kit::{App, Global, Hsla, WindowAppearance};

use crate::color::{hex, hexa};
use crate::tokens::{dark, light, mono};

/// The user's appearance choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Always light.
    Light,
    /// Always dark.
    Dark,
    /// Follow the system appearance.
    #[default]
    Automatic,
}

impl Mode {
    /// Every mode, in settings order.
    pub const ALL: [Mode; 3] = [Mode::Light, Mode::Dark, Mode::Automatic];

    /// Sentence-case label.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Light => "Light",
            Mode::Dark => "Dark",
            Mode::Automatic => "Automatic",
        }
    }

    /// The concrete appearance for this mode given the system's.
    pub fn resolve(self, system: WindowAppearance) -> Appearance {
        match self {
            Mode::Light => Appearance::Light,
            Mode::Dark => Appearance::Dark,
            Mode::Automatic => Appearance::from(system),
        }
    }
}

/// A concrete light or dark appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    /// Light window.
    #[default]
    Light,
    /// Dark window.
    Dark,
}

impl From<WindowAppearance> for Appearance {
    fn from(value: WindowAppearance) -> Self {
        match value {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
        }
    }
}

/// Persisted appearance preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeSettings {
    /// Light, dark or automatic.
    pub mode: Mode,
    /// Accent theme.
    pub accent: AccentTheme,
    /// Whether Now Playing and the mini player take their colour from the cover.
    pub tint_with_cover: bool,
    /// Which app icon the Dock shows: light, dark or following the system.
    pub app_icon: Mode,
    /// Read along text size, an index into [`READ_ALONG_SCALES`].
    pub read_along_step: u8,
}

/// Read along text sizes, as multiples of the design size.
pub const READ_ALONG_SCALES: [f32; 5] = [0.85, 1.0, 1.15, 1.3, 1.5];

/// The step [`READ_ALONG_SCALES`] starts on: the design size.
pub const READ_ALONG_DEFAULT_STEP: u8 = 1;

impl ThemeSettings {
    /// The read along text size as a multiple of the design size.
    pub fn read_along_scale(&self) -> f32 {
        READ_ALONG_SCALES[usize::from(self.read_along_step).min(READ_ALONG_SCALES.len() - 1)]
    }

    /// These settings with the read along text `delta` steps larger, clamped.
    pub fn with_read_along_step(self, delta: i8) -> Self {
        let last = (READ_ALONG_SCALES.len() - 1) as i8;
        Self {
            read_along_step: (self.read_along_step as i8 + delta).clamp(0, last) as u8,
            ..self
        }
    }
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Automatic,
            accent: AccentTheme::Ember,
            tint_with_cover: true,
            app_icon: Mode::Automatic,
            read_along_step: READ_ALONG_DEFAULT_STEP,
        }
    }
}

/// Neutral window colours for one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    /// Content background.
    pub window: Hsla,
    /// Sidebar material.
    pub sidebar: Hsla,
    /// Primary text.
    pub ink: Hsla,
    /// Secondary text and glyphs.
    pub ink_secondary: Hsla,
    /// Top of the ink button gradient (ink is the bottom).
    pub ink_top: Hsla,
    /// Text and glyphs drawn on an ink surface.
    pub on_ink: Hsla,
    /// Separators and rings.
    pub hairline: Hsla,
    /// Subtle control background.
    pub fill: Hsla,
    /// Secondary button gradient top.
    pub button_top: Hsla,
    /// Secondary button gradient bottom.
    pub button_bottom: Hsla,
    /// Floating player material.
    pub floating: Hsla,
    /// Base colour for drop shadows.
    pub shadow: Hsla,
    /// Base colour for top highlights.
    pub highlight: Hsla,
}

impl Colors {
    /// Neutral colours for `appearance`.
    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Light => Self {
                window: hex(light::WINDOW),
                sidebar: hex(light::WINDOW),
                ink: hex(light::INK),
                ink_secondary: hex(light::INK_SECONDARY),
                ink_top: hex(light::INK_TOP),
                on_ink: hex(light::WINDOW),
                hairline: hexa(light::HAIRLINE),
                fill: hexa(light::FILL),
                button_top: hex(light::BUTTON_TOP),
                button_bottom: hex(light::BUTTON_BOTTOM),
                floating: hexa(light::FLOATING),
                shadow: hex(light::INK),
                highlight: hex(mono::WHITE),
            },
            Appearance::Dark => Self {
                window: hex(dark::WINDOW),
                sidebar: hex(dark::SIDEBAR),
                ink: hex(dark::INK),
                ink_secondary: hex(dark::INK_SECONDARY),
                ink_top: hex(dark::INK_TOP),
                on_ink: hex(dark::WINDOW),
                hairline: hexa(dark::HAIRLINE),
                fill: hexa(dark::FILL),
                button_top: hex(dark::BUTTON_TOP),
                button_bottom: hex(dark::BUTTON_BOTTOM),
                floating: hexa(dark::FLOATING),
                shadow: hex(mono::BLACK),
                highlight: hex(mono::WHITE),
            },
        }
    }
}

/// The resolved theme, stored as a GPUI global.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The preferences this theme was resolved from.
    pub settings: ThemeSettings,
    /// The appearance in effect.
    pub appearance: Appearance,
    /// Neutral colours.
    pub colors: Colors,
    /// Accent roles.
    pub accent: Accent,
    /// Fallback Now Playing palette, used before a cover is analysed or when
    /// cover tinting is off.
    pub book: BookPalette,
}

impl Global for Theme {}

impl Theme {
    /// Resolves `settings` against the system appearance.
    pub fn resolve(settings: ThemeSettings, system: WindowAppearance) -> Self {
        let appearance = settings.mode.resolve(system);
        let colors = Colors::for_appearance(appearance);
        Self {
            settings,
            appearance,
            accent: Accent::resolve(settings.accent, appearance, colors.window),
            colors,
            book: BookPalette::default(),
        }
    }

    /// Installs a theme for `settings` using the current system appearance.
    pub fn init(settings: ThemeSettings, cx: &mut App) {
        let theme = Self::resolve(settings, cx.window_appearance());
        Self::sync_component_theme(theme.appearance, cx);
        cx.set_global(theme);
    }

    fn sync_component_theme(appearance: Appearance, cx: &mut App) {
        use gpui_kit::component::theme::{Theme as ComponentTheme, ThemeMode};
        let mode = match appearance {
            Appearance::Light => ThemeMode::Light,
            Appearance::Dark => ThemeMode::Dark,
        };
        ComponentTheme::change(mode, None, cx);
    }

    /// Replaces the preferences and re-resolves the theme, refreshing windows.
    pub fn apply_settings(settings: ThemeSettings, cx: &mut App) {
        Self::init(settings, cx);
        cx.refresh_windows();
    }

    /// Re-resolves after the system appearance changed (only matters in Automatic mode).
    pub fn sync_system_appearance(system: WindowAppearance, cx: &mut App) {
        let settings = cx.theme().settings;
        let theme = Self::resolve(settings, system);
        if &theme != cx.theme() {
            Self::sync_component_theme(theme.appearance, cx);
            cx.set_global(theme);
            cx.refresh_windows();
        }
    }

    /// The Now Playing palette for a book, honouring the cover-tint preference.
    pub fn book_palette(&self, cover: Option<BookPalette>) -> BookPalette {
        match cover {
            Some(palette) if self.settings.tint_with_cover => palette,
            _ => self.book,
        }
    }
}

/// Access to the resolved [`Theme`] from any GPUI context.
pub trait ActiveTheme {
    /// The current theme.
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{contrast_ratio, over};

    #[test]
    fn automatic_follows_system() {
        let settings = ThemeSettings::default();
        assert_eq!(
            Theme::resolve(settings, WindowAppearance::VibrantDark).appearance,
            Appearance::Dark
        );
        assert_eq!(
            Theme::resolve(settings, WindowAppearance::Light).appearance,
            Appearance::Light
        );
    }

    #[test]
    fn explicit_mode_ignores_system() {
        let settings = ThemeSettings {
            mode: Mode::Light,
            ..ThemeSettings::default()
        };
        assert_eq!(
            Theme::resolve(settings, WindowAppearance::Dark).appearance,
            Appearance::Light
        );
    }

    #[test]
    fn text_contrast_meets_aa_in_both_appearances() {
        for appearance in [Appearance::Light, Appearance::Dark] {
            let c = Colors::for_appearance(appearance);
            assert!(contrast_ratio(c.ink, c.window) >= 7.0, "{appearance:?} ink");
            assert!(
                contrast_ratio(c.ink_secondary, c.window) >= 4.5,
                "{appearance:?} secondary"
            );
            assert!(
                contrast_ratio(c.ink_secondary, over(c.fill, c.window)) >= 4.5,
                "{appearance:?} secondary on fill"
            );
        }
    }
}
