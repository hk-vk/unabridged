//! Accent themes and their resolved roles.

use gpui_kit::Hsla;

use super::Appearance;
use crate::color::{contrast_ratio, hex, mix, over, with_alpha};
use crate::tokens::{self, AccentTokens, mono};

/// Share of white mixed into the accent for a derived gradient top.
const DERIVED_TOP_LIFT: f32 = 0.14;
/// Share of white mixed into the accent for a derived glow tone.
const DERIVED_GLOW_LIFT: f32 = 0.4;
/// Accent opacity of the selected-row tint in dark mode.
const DARK_TINT_ALPHA: f32 = 0.2;
/// Share of white mixed into the glow for text on the dark tint.
const DARK_DEEP_LIFT: f32 = 0.3;
/// Minimum contrast for accent-coloured text (links, sidebar glyphs) on the window.
/// WCAG non-text and large-text threshold; below it the deep tone is used instead.
const ACCENT_TEXT_MIN_CONTRAST: f32 = 3.0;

/// The five accent themes offered in Appearance settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AccentTheme {
    /// Warm coral on plum ink (default).
    #[default]
    Ember,
    /// Deep teal.
    Fjord,
    /// Violet.
    Plum,
    /// Forest green.
    Moss,
    /// Golden yellow with dark text on buttons.
    Saffron,
}

impl AccentTheme {
    /// Every theme, in settings order.
    pub const ALL: [AccentTheme; 5] = [
        AccentTheme::Ember,
        AccentTheme::Fjord,
        AccentTheme::Plum,
        AccentTheme::Moss,
        AccentTheme::Saffron,
    ];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            AccentTheme::Ember => "Ember",
            AccentTheme::Fjord => "Fjord",
            AccentTheme::Plum => "Plum",
            AccentTheme::Moss => "Moss",
            AccentTheme::Saffron => "Saffron",
        }
    }

    /// One-line description shown under the name.
    pub fn note(self) -> &'static str {
        match self {
            AccentTheme::Ember => "Warm coral on plum ink",
            AccentTheme::Fjord => "Deep teal, cool and quiet",
            AccentTheme::Plum => "Violet, made for late nights",
            AccentTheme::Moss => "Forest green, easy on the eyes",
            AccentTheme::Saffron => "Golden yellow with dark text on buttons",
        }
    }

    /// The raw design tokens.
    pub fn tokens(self) -> AccentTokens {
        match self {
            AccentTheme::Ember => tokens::EMBER,
            AccentTheme::Fjord => tokens::FJORD,
            AccentTheme::Plum => tokens::PLUM,
            AccentTheme::Moss => tokens::MOSS,
            AccentTheme::Saffron => tokens::SAFFRON,
        }
    }
}

/// Accent roles resolved for one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Accent {
    /// Fill of primary buttons, progress and switches.
    pub fill: Hsla,
    /// Top of the primary button gradient.
    pub top: Hsla,
    /// Ring around primary surfaces; text on the tint in light mode.
    pub deep: Hsla,
    /// Selected-row and tile background.
    pub tint: Hsla,
    /// Text drawn on the tint.
    pub on_tint: Hsla,
    /// Light glow tone (coloured button glow, dark previews).
    pub glow: Hsla,
    /// Accent-coloured text and glyphs on the window (links, sidebar icons).
    pub text: Hsla,
    /// Text and glyphs on the accent fill.
    pub on_fill: Hsla,
}

impl Accent {
    /// Resolves `theme` for `appearance` over a `window` background.
    pub fn resolve(theme: AccentTheme, appearance: Appearance, window: Hsla) -> Self {
        let t = theme.tokens();
        let white = hex(mono::WHITE);
        let fill = hex(t.accent);
        let deep = hex(t.deep);
        let top = t
            .top
            .map(hex)
            .unwrap_or_else(|| mix(fill, white, DERIVED_TOP_LIFT));
        let glow = t
            .glow
            .map(hex)
            .unwrap_or_else(|| mix(fill, white, DERIVED_GLOW_LIFT));
        let on_fill = if t.dark_text_on_accent {
            hex(tokens::light::INK)
        } else {
            white
        };

        match appearance {
            Appearance::Light => Self {
                fill,
                top,
                deep,
                tint: hex(t.tint),
                on_tint: deep,
                glow,
                text: readable_on(fill, deep, window),
                on_fill,
            },
            Appearance::Dark => Self {
                fill,
                top,
                deep,
                tint: over(with_alpha(fill, DARK_TINT_ALPHA), window),
                on_tint: mix(glow, white, DARK_DEEP_LIFT),
                glow,
                text: readable_on(glow, fill, window),
                on_fill,
            },
        }
    }
}

/// `preferred` if it reads on `background`, otherwise `fallback`.
fn readable_on(preferred: Hsla, fallback: Hsla, background: Hsla) -> Hsla {
    if contrast_ratio(preferred, background) >= ACCENT_TEXT_MIN_CONTRAST {
        preferred
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::relative_luminance;
    use crate::theme::Colors;

    #[test]
    fn ember_uses_design_values_in_light() {
        let window = Colors::for_appearance(Appearance::Light).window;
        let ember = Accent::resolve(AccentTheme::Ember, Appearance::Light, window);
        assert_eq!(ember.fill, hex(0xD63F25));
        assert_eq!(ember.top, hex(0xEC5A3C));
        assert_eq!(ember.tint, hex(0xFCEBE6));
        assert_eq!(ember.text, ember.fill);
    }

    #[test]
    fn derived_tops_are_lighter_than_fills() {
        for theme in AccentTheme::ALL {
            let a = Accent::resolve(theme, Appearance::Light, hex(0xFFFFFF));
            assert!(
                relative_luminance(a.top) > relative_luminance(a.fill),
                "{theme:?}"
            );
            assert!(
                relative_luminance(a.glow) > relative_luminance(a.fill),
                "{theme:?}"
            );
        }
    }

    #[test]
    fn accent_text_stays_readable() {
        for appearance in [Appearance::Light, Appearance::Dark] {
            let window = Colors::for_appearance(appearance).window;
            for theme in AccentTheme::ALL {
                let a = Accent::resolve(theme, appearance, window);
                assert!(
                    contrast_ratio(a.text, window) >= ACCENT_TEXT_MIN_CONTRAST,
                    "{theme:?} {appearance:?}"
                );
                assert!(
                    contrast_ratio(a.on_tint, a.tint) >= 4.5,
                    "{theme:?} {appearance:?}"
                );
            }
        }
    }

    #[test]
    fn saffron_uses_dark_text_on_buttons() {
        let a = Accent::resolve(AccentTheme::Saffron, Appearance::Light, hex(0xFFFFFF));
        assert!(contrast_ratio(a.on_fill, a.fill) >= 4.5);
    }
}
