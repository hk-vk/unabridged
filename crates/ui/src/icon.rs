//! The icon set: Hugeicons free, stroke-rounded at 1.5 (MIT, `assets/icons/LICENSE.md`).
//!
//! Every glyph is a variant of [`Icon`]; its SVG is compiled into the binary and
//! served by [`crate::assets::Assets`]. Draw one with [`icon`], then size and tint
//! it like any element: `icon(Icon::Search).size(px(15.)).text_color(color)`.

use gpui_kit::{Svg, svg};

macro_rules! icons {
    ($($(#[$doc:meta])* $variant:ident => $file:literal,)*) => {
        /// A bundled Hugeicons glyph.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon {
            $($(#[$doc])* $variant,)*
        }

        impl Icon {
            /// Every icon.
            pub const ALL: &'static [Icon] = &[$(Icon::$variant,)*];

            /// Asset path, as served by [`crate::assets::Assets`].
            pub const fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("icons/", $file, ".svg"),)*
                }
            }

            /// The SVG source.
            pub(crate) const fn bytes(self) -> &'static [u8] {
                match self {
                    $(Icon::$variant => include_bytes!(concat!("../../../assets/icons/", $file, ".svg")),)*
                }
            }
        }
    };
}

icons! {
    /// Search (`search-01`).
    Search => "search",
    /// Explore (`compass-01`).
    Compass => "compass",
    /// Library (`library`).
    Library => "library",
    /// In progress (`clock-01`).
    Clock => "clock",
    /// Downloaded to this Mac (`download-04`).
    Download => "download",
    /// Bookmarks (`bookmark-01`).
    Bookmark => "bookmark",
    /// Add bookmark (`bookmark-add-01`).
    BookmarkAdd => "bookmark-add",
    /// Authors (`user`).
    User => "user",
    /// Series (`layers-01`).
    Layers => "layers",
    /// Finished (`checkmark-circle-02`).
    CheckmarkCircle => "checkmark-circle",
    /// Collections (`playlist-01`).
    Playlist => "playlist",
    /// Play, filled (`play`).
    Play => "play",
    /// Pause, filled (`pause`).
    Pause => "pause",
    /// Back 30 seconds (`go-backward-30-sec`).
    Back30 => "go-backward-30",
    /// Forward 30 seconds (`go-forward-30-sec`).
    Forward30 => "go-forward-30",
    /// Sleep timer (`moon-02`).
    Moon => "moon",
    /// Volume (`volume-high`).
    VolumeHigh => "volume-high",
    /// Low volume (`volume-low`).
    VolumeLow => "volume-low",
    /// AirPlay (`airplay-line`).
    Airplay => "airplay",
    /// Back (`arrow-left-01`).
    ArrowLeft => "arrow-left",
    /// More (`more-horizontal`).
    More => "more",
    /// Credits (`coins-01`).
    Coins => "coins",
    /// Expand (`arrow-expand-01`).
    Expand => "maximize",
    /// Menu bar extra (`headphones`).
    Headphones => "headphones",
    /// Wi-Fi, menu bar mock (`wifi-01`).
    Wifi => "wifi",
    /// Battery, menu bar mock (`battery-full`).
    Battery => "battery",
    /// Sort (`arrow-up-down`).
    Sort => "sort",
    /// Checkmark (`tick-02`).
    Tick => "tick",
    /// Chapters (`left-to-right-list-number`).
    Chapters => "chapters",
    /// Clear (`cancel-01`).
    Cancel => "cancel",
    /// Settings (`settings-01`).
    Settings => "settings",
}

/// An SVG element drawing `icon`. Give it a size and a `text_color`.
pub fn icon(icon: Icon) -> Svg {
    svg().path(icon.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_stroke_svg() {
        for icon in Icon::ALL {
            let source = std::str::from_utf8(icon.bytes()).expect("utf-8");
            assert!(source.starts_with("<svg"), "{icon:?}");
            assert!(source.contains("viewBox=\"0 0 24 24\""), "{icon:?}");
            assert!(icon.path().starts_with("icons/"), "{icon:?}");
        }
    }

    #[test]
    fn icon_paths_are_unique() {
        let mut paths: Vec<_> = Icon::ALL.iter().map(|i| i.path()).collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), Icon::ALL.len());
    }
}
