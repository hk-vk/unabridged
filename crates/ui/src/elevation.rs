//! Elevation: every raised surface in the app, as named levels.
//!
//! A level resolves to a [`Surface`], a recipe of
//!
//! - a **fill** (solid or a two-stop vertical gradient),
//! - an optional **ring**: a one-device-pixel border drawn *inside* the bounds,
//! - an optional **highlight** (top) and **shade** (bottom): one-device-pixel
//!   bands drawn as zero-blur inset shadows just inside the ring,
//! - an optional **inner stroke**: a one-device-pixel band all round, just inside
//!   the ring, as an inset shadow with spread (its corners stay concentric
//!   because GPUI shrinks the hole's radii by the spread),
//! - **drop shadows** with zero spread.
//!
//! Why this shape (see `docs/gpui-notes.md`, "Elevation"): the mock-up stacks
//! `inset` highlights, a `0 0 0 0.5px` spread ring and blurred drops. In GPUI a
//! spread ring keeps the element's corner radius instead of growing it by the
//! spread, and shadow bounds are snapped outward to device pixels, so a 0.5 pt
//! outer ring renders as an uneven 0 to 1 px halo with mismatched corners. A
//! border, by contrast, is snapped to whole device pixels and shares the
//! element's SDF, so it is crisp at 1x and 2x with exact radius concentricity.
//! Highlights are offset by ring + one device pixel so exactly one device pixel
//! of highlight shows beneath the ring at any scale.

use gpui_kit::{
    AbsoluteLength, Background, BoxShadow, Hsla, Styled, Window, linear_color_stop,
    linear_gradient, px,
};

use crate::color::{hex, mix, with_alpha};
use crate::theme::{Appearance, BookPalette, Theme};
use crate::tokens::mono;

/// Gradient angle that runs top to bottom (CSS convention, which GPUI follows).
const TOP_TO_BOTTOM: f32 = 180.0;
/// Diagonal used by featured banners.
const DIAGONAL: f32 = 135.0;

/// A named elevation level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Elevation {
    /// Tiny cover in the floating player.
    CoverThumb,
    /// Shelf covers.
    CoverShelf,
    /// List row covers.
    Cover,
    /// The Library hero cover.
    CoverHero,
    /// Cover on a dark book field (banner, popover, mini player).
    CoverOnField,
    /// The large Now Playing cover.
    CoverStage,
    /// Accent push button (Resume).
    ButtonPrimary,
    /// Neutral push button (Chapters, Explore new books).
    ButtonSecondary,
    /// Small neutral control without the soft drop (speed chip, credits).
    Chip,
    /// Round ink play button on the window.
    ButtonInk,
    /// Light button on a book field (play, Get with 1 credit).
    ButtonLight(BookPalette),
    /// The 72 pt Now Playing play button: same as `ButtonLight` with a deeper drop.
    ButtonLightLarge(BookPalette),
    /// Translucent secondary button on a book field (Listen to sample).
    ButtonGlass(BookPalette),
    /// Recessed text field.
    Field,
    /// Switch track, on.
    SwitchOn,
    /// Switch track, off.
    SwitchOff,
    /// Switch knob.
    Knob,
    /// The floating player bar.
    FloatingPlayer,
    /// Featured banner on Explore.
    Banner(BookPalette),
    /// Translucent book-tinted window body (mini player, menu bar popover).
    WindowMaterial(BookPalette),
}

/// The resolved recipe for one level. Apply it with [`ElevationExt::elevation`].
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    /// Background fill.
    pub fill: Option<Background>,
    /// Inside border colour.
    pub ring: Option<Hsla>,
    /// Top highlight colour.
    pub highlight: Option<Hsla>,
    /// Bottom shade colour.
    pub shade: Option<Hsla>,
    /// All-round inner stroke colour.
    pub inner_stroke: Option<Hsla>,
    /// A blurred inset shadow (recessed fields).
    pub recess: Option<BoxShadow>,
    /// Drop shadows, back to front.
    pub drops: Vec<BoxShadow>,
}

/// A drop shadow `y` points down with `blur`, in `color` at `alpha`.
fn drop(y: f32, blur: f32, color: Hsla, alpha: f32) -> BoxShadow {
    BoxShadow::new(px(0.0), px(y), with_alpha(color, alpha)).blur_radius(px(blur))
}

fn vertical(top: Hsla, bottom: Hsla) -> Option<Background> {
    Some(linear_gradient(
        TOP_TO_BOTTOM,
        linear_color_stop(top, 0.0),
        linear_color_stop(bottom, 1.0),
    ))
}

impl Elevation {
    /// Resolves this level against `theme`.
    pub fn surface(self, theme: &Theme) -> Surface {
        let c = &theme.colors;
        let a = &theme.accent;
        let dark = theme.appearance == Appearance::Dark;
        let ink = c.shadow;
        let black = hex(mono::BLACK);
        let white = hex(mono::WHITE);
        // Light-mode highlights are pure white; in dark mode they are kept faint
        // so raised controls read as lit, not outlined.
        let lit =
            |light: f32, dark_alpha: f32| with_alpha(white, if dark { dark_alpha } else { light });
        let none = Surface {
            fill: None,
            ring: None,
            highlight: None,
            shade: None,
            inner_stroke: None,
            recess: None,
            drops: Vec::new(),
        };
        let cover = |drops: Vec<BoxShadow>| Surface {
            ring: Some(with_alpha(black, 0.1)),
            drops,
            ..none.clone()
        };

        match self {
            Elevation::CoverThumb => cover(vec![drop(1.0, 3.0, black, 0.25)]),
            Elevation::CoverShelf => {
                cover(vec![drop(1.0, 2.0, ink, 0.1), drop(6.0, 16.0, ink, 0.12)])
            }
            Elevation::Cover => cover(vec![drop(1.0, 2.0, ink, 0.12), drop(8.0, 20.0, ink, 0.16)]),
            Elevation::CoverHero => {
                cover(vec![drop(1.0, 2.0, ink, 0.12), drop(12.0, 32.0, ink, 0.18)])
            }
            Elevation::CoverOnField => cover(vec![
                drop(2.0, 4.0, black, 0.3),
                drop(16.0, 36.0, black, 0.4),
            ]),
            Elevation::CoverStage => cover(vec![
                drop(2.0, 4.0, black, 0.25),
                drop(30.0, 70.0, black, 0.45),
            ]),
            Elevation::ButtonPrimary => Surface {
                fill: vertical(a.top, a.fill),
                ring: Some(with_alpha(a.deep, 0.9)),
                highlight: Some(lit(0.4, 0.3)),
                shade: Some(with_alpha(mix(a.deep, black, 0.25), 0.25)),
                drops: vec![drop(1.0, 2.0, a.deep, 0.35), drop(6.0, 14.0, a.fill, 0.32)],
                ..none
            },
            Elevation::ButtonSecondary | Elevation::Chip => Surface {
                fill: vertical(c.button_top, c.button_bottom),
                ring: Some(with_alpha(
                    if dark { black } else { ink },
                    if dark { 0.5 } else { 0.14 },
                )),
                highlight: Some(lit(1.0, 0.1)),
                drops: if self == Elevation::Chip {
                    vec![drop(1.0, 2.0, ink, 0.08)]
                } else {
                    vec![drop(1.0, 2.0, ink, 0.08), drop(3.0, 8.0, ink, 0.06)]
                },
                ..none
            },
            Elevation::ButtonInk => Surface {
                fill: vertical(c.ink_top, c.ink),
                ring: Some(with_alpha(c.ink, 0.9)),
                highlight: Some(lit(0.22, 0.6)),
                drops: vec![drop(2.0, 4.0, ink, 0.25), drop(6.0, 14.0, ink, 0.28)],
                ..none
            },
            Elevation::ButtonLight(book) | Elevation::ButtonLightLarge(book) => Surface {
                fill: vertical(book.button_top(), book.button_bottom),
                ring: Some(with_alpha(black, 0.35)),
                highlight: Some(white),
                drops: if matches!(self, Elevation::ButtonLightLarge(_)) {
                    vec![drop(2.0, 4.0, black, 0.25), drop(12.0, 28.0, black, 0.38)]
                } else {
                    vec![drop(2.0, 4.0, black, 0.25), drop(6.0, 14.0, black, 0.3)]
                },
                ..none
            },
            Elevation::ButtonGlass(book) => Surface {
                fill: Some(book.on_field_track.into()),
                ring: Some(with_alpha(white, 0.2)),
                highlight: Some(with_alpha(white, 0.18)),
                ..none
            },
            Elevation::Field => Surface {
                fill: Some(c.window.into()),
                ring: Some(with_alpha(
                    if dark { white } else { ink },
                    if dark { 0.16 } else { 0.2 },
                )),
                recess: Some(
                    BoxShadow::new(
                        px(0.0),
                        px(1.0),
                        with_alpha(ink, if dark { 0.3 } else { 0.08 }),
                    )
                    .blur_radius(px(2.0))
                    .inset(),
                ),
                drops: vec![drop(1.0, 1.0, ink, 0.04)],
                ..none
            },
            Elevation::SwitchOn => Surface {
                fill: vertical(a.top, a.fill),
                ring: Some(with_alpha(a.deep, 0.8)),
                recess: Some(
                    BoxShadow::new(px(0.0), px(1.0), with_alpha(a.deep, 0.4))
                        .blur_radius(px(2.0))
                        .inset(),
                ),
                ..none
            },
            Elevation::SwitchOff => Surface {
                fill: Some(c.fill.into()),
                ring: Some(c.hairline),
                recess: Some(
                    BoxShadow::new(px(0.0), px(1.0), with_alpha(ink, 0.12))
                        .blur_radius(px(2.0))
                        .inset(),
                ),
                ..none
            },
            Elevation::Knob => Surface {
                fill: vertical(hex(mono::WHITE), hex(crate::tokens::light::BUTTON_BOTTOM)),
                ring: Some(with_alpha(ink, 0.2)),
                drops: vec![drop(1.0, 2.0, ink, 0.3), drop(3.0, 6.0, ink, 0.18)],
                ..none
            },
            Elevation::FloatingPlayer => Surface {
                fill: Some(c.floating.into()),
                ring: Some(with_alpha(
                    if dark { black } else { ink },
                    if dark { 0.6 } else { 0.12 },
                )),
                inner_stroke: Some(lit(0.85, 0.08)),
                drops: vec![
                    drop(1.0, 2.0, ink, 0.06),
                    drop(8.0, 20.0, ink, 0.12),
                    drop(24.0, 48.0, ink, 0.14),
                ],
                ..none
            },
            Elevation::Banner(book) => Surface {
                fill: Some(linear_gradient(
                    DIAGONAL,
                    linear_color_stop(book.field, 0.0),
                    linear_color_stop(book.field_deep, 1.0),
                )),
                ring: Some(with_alpha(ink, 0.4)),
                drops: vec![drop(2.0, 4.0, ink, 0.12), drop(16.0, 36.0, ink, 0.22)],
                ..none
            },
            Elevation::WindowMaterial(book) => Surface {
                fill: Some(book.material().into()),
                ring: Some(with_alpha(black, 0.5)),
                inner_stroke: Some(with_alpha(white, 0.14)),
                ..none
            },
        }
    }
}

impl Surface {
    /// Applies the recipe to `element`. `window` supplies the device pixel size.
    pub fn apply<E: Styled>(&self, mut element: E, window: &Window) -> E {
        let device_px = 1.0 / window.scale_factor();
        let ring_width = if self.ring.is_some() { device_px } else { 0.0 };
        let band = ring_width + device_px;

        if let Some(fill) = self.fill {
            element = element.bg(fill);
        }

        let mut shadows = self.drops.clone();
        if let Some(color) = self.highlight {
            shadows.push(BoxShadow::new(px(0.0), px(band), color).inset());
        }
        if let Some(color) = self.shade {
            shadows.push(BoxShadow::new(px(0.0), px(-band), color).inset());
        }
        if let Some(color) = self.inner_stroke {
            shadows.push(
                BoxShadow::new(px(0.0), px(0.0), color)
                    .spread_radius(px(band))
                    .inset(),
            );
        }
        if let Some(recess) = &self.recess {
            shadows.push(recess.clone());
        }
        if !shadows.is_empty() {
            element = element.shadow(shadows);
        }

        if let Some(ring) = self.ring {
            element = element.border_color(ring);
            let width = AbsoluteLength::Pixels(px(ring_width));
            let widths = &mut element.style().border_widths;
            widths.top = Some(width);
            widths.right = Some(width);
            widths.bottom = Some(width);
            widths.left = Some(width);
        }
        element
    }
}

/// Applies an [`Elevation`] to any styled element.
pub trait ElevationExt: Styled + Sized {
    /// Styles the element as `level` in `theme`.
    fn elevation(self, level: Elevation, theme: &Theme, window: &Window) -> Self {
        level.surface(theme).apply(self, window)
    }
}

impl<T: Styled> ElevationExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeSettings;
    use gpui_kit::WindowAppearance;

    fn levels() -> Vec<Elevation> {
        let book = BookPalette::default();
        vec![
            Elevation::CoverThumb,
            Elevation::CoverShelf,
            Elevation::Cover,
            Elevation::CoverHero,
            Elevation::CoverOnField,
            Elevation::CoverStage,
            Elevation::ButtonPrimary,
            Elevation::ButtonSecondary,
            Elevation::Chip,
            Elevation::ButtonInk,
            Elevation::ButtonLight(book),
            Elevation::ButtonLightLarge(book),
            Elevation::ButtonGlass(book),
            Elevation::Field,
            Elevation::SwitchOn,
            Elevation::SwitchOff,
            Elevation::Knob,
            Elevation::FloatingPlayer,
            Elevation::Banner(book),
            Elevation::WindowMaterial(book),
        ]
    }

    #[test]
    fn no_level_uses_spread_or_inset_drops() {
        for appearance in [WindowAppearance::Light, WindowAppearance::Dark] {
            let theme = Theme::resolve(ThemeSettings::default(), appearance);
            for level in levels() {
                let surface = level.surface(&theme);
                for shadow in &surface.drops {
                    assert!(!shadow.inset, "{level:?}: drops must be outside");
                    assert_eq!(
                        shadow.spread_radius,
                        px(0.0),
                        "{level:?}: spread rings render unevenly"
                    );
                }
                if let Some(recess) = &surface.recess {
                    assert!(recess.inset, "{level:?}: recess must be inset");
                }
            }
        }
    }

    #[test]
    fn highlights_and_strokes_sit_inside_a_ring() {
        let theme = Theme::resolve(ThemeSettings::default(), WindowAppearance::Light);
        for level in levels() {
            let s = level.surface(&theme);
            if s.highlight.is_some() || s.shade.is_some() || s.inner_stroke.is_some() {
                assert!(s.ring.is_some(), "{level:?}: highlight without a ring edge");
            }
        }
    }
}
