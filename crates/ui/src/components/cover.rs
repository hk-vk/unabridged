//! Book cover art with the cover elevation.

use gpui_kit::{
    App, ImageSource, IntoElement, ObjectFit, ParentElement, RenderOnce, Resource, Styled,
    StyledImage as _, Window, div, img, px, relative,
};

use crate::elevation::{Elevation, ElevationExt};
use crate::theme::ActiveTheme;

/// How a [`Cover`] is sized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CoverSize {
    /// A square of the given side.
    Square(f32),
    /// Full width of the parent at a fixed height (shelf grids).
    FillWidth {
        /// Height in points.
        height: f32,
    },
}

/// Cover art: an image cropped to fill, with rounded corners, a hairline edge
/// and the shadow of its elevation level.
#[derive(IntoElement)]
pub struct Cover {
    source: ImageSource,
    size: CoverSize,
    radius: f32,
    elevation: Elevation,
}

impl Cover {
    /// A cover loaded from `source` (an asset path, URL or file path).
    pub fn new(source: impl Into<ImageSource>, size: CoverSize, radius: f32) -> Self {
        Self {
            source: source.into(),
            size,
            radius,
            elevation: Elevation::Cover,
        }
    }

    /// Sets the elevation level (one of the `Cover*` levels).
    pub fn elevation(mut self, level: Elevation) -> Self {
        self.elevation = level;
        self
    }
}

impl RenderOnce for Cover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let frame = div().flex_none().rounded(px(self.radius));
        let frame = match self.size {
            CoverSize::Square(side) => frame.size(px(side)),
            CoverSize::FillWidth { height } => frame.w(relative(1.0)).h(px(height)),
        };
        // Covers on disk go through the cover cache, decoded at the size drawn
        // and freed once off screen; see `cover_cache`.
        let source = match &self.source {
            ImageSource::Resource(Resource::Path(path)) if !path.as_os_str().is_empty() => {
                let path = path.clone();
                let pixels = match self.size {
                    // Rounded up to 64 px steps, so a cover whose size animates
                    // (the mini player) reuses a few decodes, not one a frame.
                    CoverSize::Square(side) => {
                        ((side * window.scale_factor()).ceil() as u32).div_ceil(64) * 64
                    }
                    CoverSize::FillWidth { .. } => 0,
                };
                ImageSource::Custom(std::sync::Arc::new(move |window, cx| {
                    super::cover_cache::load(&path, pixels, window, cx)
                }))
            }
            _ => self.source,
        };
        // The ring is a border, which GPUI paints above children, so it outlines
        // the image edge; the image carries the same radius to clip its corners.
        frame
            .bg(cx.theme().colors.fill)
            .elevation(self.elevation, cx.theme(), window)
            .child(
                img(source)
                    .size_full()
                    .rounded(px(self.radius))
                    .object_fit(ObjectFit::Cover),
            )
    }
}
