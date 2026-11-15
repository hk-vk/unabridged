//! Now Playing colours derived from a book cover.
//!
//! [`BookPalette::dominant_color`] finds the cover's characteristic colour;
//! [`BookPalette::from_cover_color`] turns it into a dark field with light text
//! that always clears WCAG AA (4.5:1), including the muted secondary text.

use gpui_kit::{Hsla, hsla};

use crate::color::{contrast_ratio, hex, over, with_alpha};
use crate::tokens::{book, mono};

/// Bits kept per channel when bucketing pixels (4 bits: 4096 buckets).
const QUANT_BITS: u32 = 4;
/// Pixels with alpha below this are ignored.
const MIN_ALPHA: u8 = 128;
/// Weight multiplier for saturated pixels, so a coloured cover beats its white border.
const SATURATION_WEIGHT: f32 = 3.0;
/// Weight for near-black and near-white pixels, which say little about the cover.
const EXTREME_WEIGHT: f32 = 0.2;
/// Lightness below/above which a pixel counts as near-black/near-white.
const EXTREME_LIGHTNESS: (f32, f32) = (0.08, 0.92);

/// Field lightness (the design's `#232A44` sits at about 0.20).
const FIELD_LIGHTNESS: f32 = 0.2;
/// Deep field lightness (`#161B2E`, about 0.13).
const FIELD_DEEP_LIGHTNESS: f32 = 0.13;
/// Saturation range for the field: enough to carry the cover's hue, never garish.
const FIELD_SATURATION: (f32, f32) = (0.08, 0.42);
/// Saturation and lightness of the near-white text tinted with the cover hue.
const ON_FIELD_SL: (f32, f32) = (0.2, 0.95);
/// Saturation and lightness of the light play button's gradient bottom.
const BUTTON_BOTTOM_SL: (f32, f32) = (0.2, 0.86);
/// Required contrast for text on the field.
const MIN_TEXT_CONTRAST: f32 = 4.5;
/// Lightness removed per step while enforcing contrast.
const DARKEN_STEP: f32 = 0.02;

/// The colours Now Playing, the mini player and the menu bar popover draw with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BookPalette {
    /// Gradient top of the field.
    pub field: Hsla,
    /// Gradient bottom of the field.
    pub field_deep: Hsla,
    /// Primary text on the field.
    pub on_field: Hsla,
    /// Secondary text on the field.
    pub on_field_muted: Hsla,
    /// Tracks, outlines and hairlines on the field.
    pub on_field_track: Hsla,
    /// Bottom of the light play button gradient (top is white).
    pub button_bottom: Hsla,
}

impl Default for BookPalette {
    /// The design's default sample palette (`book/*` tokens).
    fn default() -> Self {
        let on_field = hex(book::ON_FIELD);
        Self {
            field: hex(book::FIELD),
            field_deep: hex(book::FIELD_DEEP),
            on_field,
            on_field_muted: with_alpha(on_field, book::MUTED_ALPHA),
            on_field_track: with_alpha(on_field, book::TRACK_ALPHA),
            button_bottom: hex(book::BUTTON_BOTTOM),
        }
    }
}

impl BookPalette {
    /// A palette in the hue of `cover`, its dominant colour.
    pub fn from_cover_color(cover: Hsla) -> Self {
        let hue = cover.h;
        let saturation = cover.s.clamp(FIELD_SATURATION.0, FIELD_SATURATION.1);
        let on_field = hsla(hue, ON_FIELD_SL.0, ON_FIELD_SL.1, 1.0);
        let on_field_muted = with_alpha(on_field, book::MUTED_ALPHA);

        // Darken until the weakest text (muted, composited on the lighter field
        // colour) clears AA. With these lightness targets this rarely loops.
        let mut lightness = FIELD_LIGHTNESS;
        let mut field = hsla(hue, saturation, lightness, 1.0);
        while contrast_ratio(over(on_field_muted, field), field) < MIN_TEXT_CONTRAST
            && lightness > DARKEN_STEP
        {
            lightness -= DARKEN_STEP;
            field = hsla(hue, saturation, lightness, 1.0);
        }
        let deep_lightness =
            FIELD_DEEP_LIGHTNESS.min(lightness * FIELD_DEEP_LIGHTNESS / FIELD_LIGHTNESS);

        Self {
            field,
            field_deep: hsla(hue, saturation, deep_lightness, 1.0),
            on_field,
            on_field_muted,
            on_field_track: with_alpha(on_field, book::TRACK_ALPHA),
            button_bottom: hsla(hue, BUTTON_BOTTOM_SL.0, BUTTON_BOTTOM_SL.1, 1.0),
        }
    }

    /// The characteristic colour of an RGBA8 image, or `None` if it has no opaque pixels.
    ///
    /// Pixels are bucketed by quantised colour; buckets are weighted towards
    /// saturated, mid-lightness pixels; the heaviest bucket's mean is returned.
    pub fn dominant_color(rgba: &[u8]) -> Option<Hsla> {
        const BUCKETS: usize = 1 << (QUANT_BITS * 3);
        #[derive(Clone, Copy, Default)]
        struct Bucket {
            weight: f32,
            sum: [f32; 3],
            count: u32,
        }
        let mut buckets = vec![Bucket::default(); BUCKETS];
        let shift = 8 - QUANT_BITS;

        for px in rgba.chunks_exact(4) {
            if px[3] < MIN_ALPHA {
                continue;
            }
            let [r, g, b] = [px[0], px[1], px[2]];
            let color: Hsla = gpui_kit::Rgba {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: 1.0,
            }
            .into();
            let weight = if color.l < EXTREME_LIGHTNESS.0 || color.l > EXTREME_LIGHTNESS.1 {
                EXTREME_WEIGHT
            } else {
                1.0 + color.s * SATURATION_WEIGHT
            };
            let index = (usize::from(r >> shift) << (QUANT_BITS * 2))
                | (usize::from(g >> shift) << QUANT_BITS)
                | usize::from(b >> shift);
            let bucket = &mut buckets[index];
            bucket.weight += weight;
            bucket.sum[0] += f32::from(r);
            bucket.sum[1] += f32::from(g);
            bucket.sum[2] += f32::from(b);
            bucket.count += 1;
        }

        let best = buckets
            .iter()
            .filter(|b| b.count > 0)
            .max_by(|a, b| a.weight.total_cmp(&b.weight))?;
        let n = best.count as f32 * 255.0;
        Some(
            gpui_kit::Rgba {
                r: best.sum[0] / n,
                g: best.sum[1] / n,
                b: best.sum[2] / n,
                a: 1.0,
            }
            .into(),
        )
    }

    /// The translucent material behind the mini player and popover.
    pub fn material(&self) -> Hsla {
        with_alpha(self.field, book::MATERIAL_ALPHA)
    }

    /// Top of the light play button gradient.
    pub fn button_top(&self) -> Hsla {
        hex(mono::WHITE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::Rgba;

    fn image(pixels: &[(u32, usize)]) -> Vec<u8> {
        let mut out = Vec::new();
        for &(rgb, count) in pixels {
            for _ in 0..count {
                out.extend_from_slice(&[(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]);
            }
        }
        out
    }

    fn close(a: Hsla, b: Hsla) -> bool {
        let (a, b) = (Rgba::from(a), Rgba::from(b));
        (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
    }

    #[test]
    fn dominant_of_solid_image_is_that_colour() {
        let color = BookPalette::dominant_color(&image(&[(0x3366AA, 100)])).unwrap();
        assert!(close(color, hex(0x3366AA)));
    }

    #[test]
    fn saturated_colour_beats_a_larger_white_border() {
        let pixels = image(&[(0xFFFFFF, 70), (0x2255CC, 30)]);
        let color = BookPalette::dominant_color(&pixels).unwrap();
        assert!(close(color, hex(0x2255CC)));
    }

    #[test]
    fn transparent_image_has_no_dominant_colour() {
        assert!(BookPalette::dominant_color(&[0, 0, 0, 0]).is_none());
        assert!(BookPalette::dominant_color(&[]).is_none());
    }

    #[test]
    fn derived_palettes_keep_text_contrast() {
        for cover in [
            0xFFEE00, 0x00FF88, 0xFFFFFF, 0x000000, 0xC0392B, 0x7F8C8D, 0x232A44,
        ] {
            let p = BookPalette::from_cover_color(hex(cover));
            for field in [p.field, p.field_deep] {
                assert!(
                    contrast_ratio(p.on_field, field) >= MIN_TEXT_CONTRAST,
                    "{cover:06x}"
                );
                assert!(
                    contrast_ratio(over(p.on_field_muted, field), field) >= MIN_TEXT_CONTRAST,
                    "{cover:06x} muted"
                );
            }
        }
    }

    #[test]
    fn derived_field_keeps_cover_hue_and_deep_is_darker() {
        let cover = hex(0x2255CC);
        let p = BookPalette::from_cover_color(cover);
        assert!((p.field.h - cover.h).abs() < 1e-3);
        assert!(p.field_deep.l < p.field.l);
    }

    #[test]
    fn default_palette_matches_design_tokens() {
        let p = BookPalette::default();
        assert_eq!(p.field, hex(0x232A44));
        assert!(contrast_ratio(over(p.on_field_muted, p.field), p.field) >= MIN_TEXT_CONTRAST);
    }
}
