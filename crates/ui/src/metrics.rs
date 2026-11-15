//! Shape and size tokens shared across components and screens.
//!
//! Values are in logical points, as the design specifies them.

/// Corner radii.
pub mod radius {
    /// App windows.
    pub const WINDOW: f32 = 14.0;
    /// Push buttons (rounded rectangles, not pills).
    pub const BUTTON: f32 = 10.0;
    /// Search field.
    pub const FIELD: f32 = 10.0;
    /// Chips and small secondary buttons.
    pub const CHIP: f32 = 8.0;
    /// Sidebar rows.
    pub const ROW: f32 = 8.0;
    /// Toolbar popup buttons (sort control).
    pub const POPUP: f32 = 7.0;
    /// Keyboard shortcut hints.
    pub const KEY_HINT: f32 = 5.0;
    /// Small covers on shelves.
    pub const COVER_SMALL: f32 = 5.0;
    /// Standard covers.
    pub const COVER: f32 = 6.0;
    /// Large covers (Now Playing, banner, mini player).
    pub const COVER_LARGE: f32 = 8.0;
    /// Genre tiles and mode previews.
    pub const TILE: f32 = 12.0;
    /// Grouped lists and settings cards.
    pub const CARD: f32 = 14.0;
    /// Mini player window.
    pub const MINI_PLAYER: f32 = 16.0;
    /// Floating player bar and featured banners.
    pub const FLOATING: f32 = 18.0;
    /// Segments inside the accent chips in settings.
    pub const SWATCH: f32 = 3.0;
}

/// Control heights and fixed sizes.
pub mod size {
    /// Regular push button height.
    pub const BUTTON: f32 = 36.0;
    /// Toolbar button height (Explore, credits).
    pub const TOOLBAR_BUTTON: f32 = 30.0;
    /// Toolbar popup height (sort control).
    pub const POPUP: f32 = 28.0;
    /// Chip height (speed chip).
    pub const CHIP: f32 = 26.0;
    /// Outlined pill controls on the Now Playing field.
    pub const FIELD_PILL: f32 = 32.0;
    /// Search field height.
    pub const SEARCH_HEIGHT: f32 = 34.0;
    /// Search field width.
    pub const SEARCH_WIDTH: f32 = 320.0;
    /// Key hint height.
    pub const KEY_HINT: f32 = 20.0;
    /// Sidebar row height.
    pub const SIDEBAR_ROW: f32 = 28.0;
    /// Title bar and toolbar height.
    pub const TOOLBAR: f32 = 52.0;
    /// Sidebar width.
    pub const SIDEBAR: f32 = 220.0;
    /// Switch track.
    pub const SWITCH: (f32, f32) = (40.0, 24.0);
    /// Switch knob.
    pub const SWITCH_KNOB: f32 = 20.0;
    /// Scrubber track thickness.
    pub const SCRUBBER_TRACK: f32 = 4.0;
    /// Scrubber knob.
    pub const SCRUBBER_KNOB: f32 = 10.0;
    /// Segmented chapter progress thickness.
    pub const SEGMENTS: f32 = 5.0;
    /// Gap between chapter progress segments.
    pub const SEGMENT_GAP: f32 = 3.0;
}

/// Icon sizes.
pub mod icon {
    /// Toolbar popup glyphs.
    pub const XSMALL: f32 = 13.0;
    /// Button glyphs (Explore, credits).
    pub const SMALL: f32 = 14.0;
    /// Sidebar and search glyphs.
    pub const MEDIUM: f32 = 15.0;
    /// Player utility glyphs.
    pub const LARGE: f32 = 16.0;
}

/// Radius of a shape inset by `inset` inside one with radius `outer`, so the two
/// curves stay concentric. Never negative.
pub fn concentric(outer: f32, inset: f32) -> f32 {
    (outer - inset).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concentric_radius_subtracts_inset_and_clamps() {
        assert_eq!(concentric(radius::FLOATING, 1.0), 17.0);
        assert_eq!(concentric(4.0, 6.0), 0.0);
    }
}
