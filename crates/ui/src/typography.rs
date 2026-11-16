//! Type families, the type scale and font registration.
//!
//! Every text style in the app is a named [`TypeStyle`] constant in [`text`],
//! applied with [`TypographyExt::type_style`]. GPUI has no letter-spacing, so the
//! design's negative tracking on display sizes is not reproduced (see
//! `docs/gpui-notes.md`).

use std::borrow::Cow;
use std::sync::Arc;

use gpui_kit::{App, FontFeatures, FontWeight, Styled, px, relative};

/// Font family names as the bundled files report them.
pub mod family {
    /// Interface text: Allo (400, 500, 600).
    pub const UI: &str = "Allo";
    /// Book titles and listening sentences: Literata.
    pub const BOOK: &str = "Literata";
    /// Time readouts: Geist Pixel (Square).
    pub const PIXEL: &str = "Geist Pixel";
}

/// Line height used where the design says `normal` for interface text.
const UI_LEADING: f32 = 1.25;

/// A complete text style: family, size, weight, leading and figure style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    /// Font family name.
    pub family: &'static str,
    /// Size in points.
    pub size: f32,
    /// Weight.
    pub weight: FontWeight,
    /// Line height as a multiple of the size.
    pub leading: f32,
    /// Whether digits use tabular (fixed-width) figures.
    pub tabular: bool,
}

impl TypeStyle {
    /// Interface text (Allo) at `size`, regular weight.
    pub const fn ui(size: f32) -> Self {
        Self {
            family: family::UI,
            size,
            weight: FontWeight::NORMAL,
            leading: UI_LEADING,
            tabular: false,
        }
    }

    /// Literata at `size` with the given leading.
    pub const fn book(size: f32, leading: f32) -> Self {
        Self {
            family: family::BOOK,
            size,
            weight: FontWeight::NORMAL,
            leading,
            tabular: false,
        }
    }

    /// Geist Pixel at `size` with the given leading; always tabular.
    pub const fn pixel(size: f32, leading: f32) -> Self {
        Self {
            family: family::PIXEL,
            size,
            weight: FontWeight::NORMAL,
            leading,
            tabular: true,
        }
    }

    /// Medium (500) weight.
    pub const fn medium(self) -> Self {
        Self {
            weight: FontWeight::MEDIUM,
            ..self
        }
    }

    /// Semibold (600) weight.
    pub const fn semibold(self) -> Self {
        Self {
            weight: FontWeight::SEMIBOLD,
            ..self
        }
    }

    /// Tabular figures, for times and counters that update in place.
    pub const fn tabular(self) -> Self {
        Self {
            tabular: true,
            ..self
        }
    }
}

/// The type scale, named by role. Sizes and weights are the design's.
pub mod text {
    use super::TypeStyle;

    /// Toolbar page title (17, semibold).
    pub const PAGE_TITLE: TypeStyle = TypeStyle::ui(17.0).semibold();
    /// Shelf and section headings (20, semibold).
    pub const SECTION_TITLE: TypeStyle = TypeStyle::ui(20.0).semibold();
    /// Settings group headings (15, semibold).
    pub const GROUP_TITLE: TypeStyle = TypeStyle::ui(15.0).semibold();
    /// Lead text: hero credit, summary lines (15).
    pub const LEAD: TypeStyle = TypeStyle::ui(15.0);
    /// Banner blurb (15, 1.45 leading).
    pub const BLURB: TypeStyle = TypeStyle {
        leading: 1.45,
        ..TypeStyle::ui(15.0)
    };
    /// Row titles in settings and tiles (14, medium).
    pub const LABEL_STRONG: TypeStyle = TypeStyle::ui(14.0).medium();
    /// Secondary lines at 14 (credits on the field).
    pub const CALLOUT: TypeStyle = TypeStyle::ui(14.0);
    /// Default UI text (13).
    pub const BODY: TypeStyle = TypeStyle::ui(13.0);
    /// Emphasised UI text (13, medium): shelf titles, secondary buttons.
    pub const BODY_MEDIUM: TypeStyle = TypeStyle::ui(13.0).medium();
    /// Primary button labels (13, semibold).
    pub const BODY_STRONG: TypeStyle = TypeStyle::ui(13.0).semibold();
    /// Captions (12).
    pub const CAPTION: TypeStyle = TypeStyle::ui(12.0);
    /// Emphasised captions and chips (12, medium).
    pub const CAPTION_MEDIUM: TypeStyle = TypeStyle::ui(12.0).medium();
    /// Small print: footnotes, scrubber times (11).
    pub const FOOTNOTE: TypeStyle = TypeStyle::ui(11.0);
    /// Sidebar group headers and key hints (11, semibold).
    pub const FOOTNOTE_STRONG: TypeStyle = TypeStyle::ui(11.0).semibold();
    /// Key hint (11, medium).
    pub const KEY_HINT: TypeStyle = TypeStyle::ui(11.0).medium();

    /// Library hero title (Literata 44 / 1.05).
    pub const BOOK_DISPLAY: TypeStyle = TypeStyle::book(44.0, 1.05);
    /// Explore banner title (Literata 38 / 1.05).
    pub const BOOK_BANNER: TypeStyle = TypeStyle::book(38.0, 1.05);
    /// Now Playing title (Literata 24 / 1.2).
    pub const BOOK_TITLE: TypeStyle = TypeStyle::book(24.0, 1.2);
    /// In progress row title (Literata 20 / 1.15).
    pub const BOOK_ROW: TypeStyle = TypeStyle::book(20.0, 1.15);
    /// Now Playing caption under the numeral (Literata 19 / 1.4).
    pub const BOOK_CAPTION: TypeStyle = TypeStyle::book(19.0, 1.4);
    /// The listening sentence in the Library hero (Literata 17 / 1.45).
    pub const BOOK_SENTENCE: TypeStyle = TypeStyle::book(17.0, 1.45);
    /// Popover title (Literata 16 / 1.2).
    pub const BOOK_SMALL: TypeStyle = TypeStyle::book(16.0, 1.2);
    /// Read along in the mini player (Literata 18 / 1.38).
    pub const BOOK_READ_ALONG: TypeStyle = TypeStyle::book(18.0, 1.38);
    /// Mini player title (Literata 15 / 1.2).
    pub const BOOK_MINI: TypeStyle = TypeStyle::book(15.0, 1.2);

    /// Now Playing time-left numeral (Geist Pixel 168 / 0.95).
    pub const PIXEL_HERO: TypeStyle = TypeStyle::pixel(168.0, 0.95);
    /// In progress percentage (Geist Pixel 30 / 1).
    pub const PIXEL_STAT: TypeStyle = TypeStyle::pixel(30.0, 1.0);
    /// Menu bar time (Geist Pixel 13).
    pub const PIXEL_MENU: TypeStyle = TypeStyle::pixel(13.0, 1.25);
}

/// Applies a [`TypeStyle`] to any styled element.
pub trait TypographyExt: Styled + Sized {
    /// Sets family, size, weight, leading and figure style in one call.
    fn type_style(self, style: TypeStyle) -> Self {
        let el = self
            .font_family(style.family)
            .text_size(px(style.size))
            .font_weight(style.weight)
            .line_height(relative(style.leading));
        if style.tabular {
            el.font_features(tabular_figures())
        } else {
            el
        }
    }
}

impl<T: Styled> TypographyExt for T {}

/// OpenType features enabling tabular figures.
pub fn tabular_figures() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// The bundled font files (OFL; licences next to them in `assets/fonts`).
const FONT_FILES: [&[u8]; 5] = [
    include_bytes!("../../../assets/fonts/allo/Allo-Regular.otf"),
    include_bytes!("../../../assets/fonts/allo/Allo-Medium.otf"),
    include_bytes!("../../../assets/fonts/allo/Allo-SemiBold.otf"),
    include_bytes!("../../../assets/fonts/geist-pixel/GeistPixel-Square.ttf"),
    include_bytes!("../../../assets/fonts/literata/Literata-Regular.ttf"),
];

/// Registers the bundled fonts with GPUI's text system.
pub fn register_fonts(cx: &mut App) -> anyhow::Result<()> {
    cx.text_system().add_fonts(
        FONT_FILES
            .iter()
            .map(|bytes| Cow::Borrowed(*bytes))
            .collect(),
    )
}
