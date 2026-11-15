//! Colour helpers: hex conversion, blending and WCAG contrast.
//!
//! All arithmetic happens on gamma-encoded sRGB [`Rgba`] values, the space the
//! design file speaks in, so a token mixed here matches the same mix in the
//! mock-up.

use gpui_kit::{Hsla, Rgba, rgb, rgba};

/// An opaque colour from a `0xRRGGBB` literal.
pub fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// A colour with alpha from a `0xRRGGBBAA` literal.
pub fn hexa(value: u32) -> Hsla {
    rgba(value).into()
}

/// `color` with its alpha replaced by `alpha`.
pub fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

/// Linear interpolation between `from` and `to` in sRGB; `amount` 0 is `from`, 1 is `to`.
pub fn mix(from: Hsla, to: Hsla, amount: f32) -> Hsla {
    let (a, b) = (Rgba::from(from), Rgba::from(to));
    let t = amount.clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
    .into()
}

/// Composites a possibly translucent `top` colour over an opaque `bottom` one.
pub fn over(top: Hsla, bottom: Hsla) -> Hsla {
    let alpha = top.a.clamp(0.0, 1.0);
    let opaque_top = with_alpha(top, 1.0);
    with_alpha(mix(bottom, opaque_top, alpha), bottom.a)
}

/// WCAG 2.x relative luminance of an opaque colour.
pub fn relative_luminance(color: Hsla) -> f32 {
    let c = Rgba::from(color);
    let channel = |v: f32| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG 2.x contrast ratio between two opaque colours, from 1 to 21.
pub fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (light, dark) = if la > lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn contrast_of_black_on_white_is_21() {
        assert!(approx(contrast_ratio(hex(0x000000), hex(0xFFFFFF)), 21.0));
        assert!(approx(contrast_ratio(hex(0xFFFFFF), hex(0xFFFFFF)), 1.0));
    }

    #[test]
    fn ink_on_window_passes_aa() {
        assert!(contrast_ratio(hex(0x241E2B), hex(0xFFFFFF)) > 15.0);
        assert!(contrast_ratio(hex(0x6B6475), hex(0xFFFFFF)) >= 4.5);
    }

    #[test]
    fn mix_endpoints_and_midpoint() {
        let (black, white) = (hex(0x000000), hex(0xFFFFFF));
        assert_eq!(Rgba::from(mix(black, white, 0.0)), Rgba::from(black));
        let mid = Rgba::from(mix(black, white, 0.5));
        assert!(approx(mid.r, 0.5) && approx(mid.g, 0.5) && approx(mid.b, 0.5));
    }

    #[test]
    fn over_composites_alpha() {
        let half_black = hexa(0x00000080);
        let result = Rgba::from(over(half_black, hex(0xFFFFFF)));
        assert!(approx(result.r, 0.5));
        assert!(approx(result.a, 1.0));
    }
}
