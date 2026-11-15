//! Progress displays: the seekable scrubber and chapter-segmented progress.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, BoxShadow, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, Styled, Window, canvas, div, prelude::FluentBuilder as _,
    px, relative,
};

use crate::color::{hex, with_alpha};
use crate::metrics::size;
use crate::theme::{ActiveTheme, BookPalette};
use crate::tokens::mono;

/// Seek handler: receives the fraction (0..=1) the user picked.
pub type SeekHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// Colours of a [`Scrubber`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrubberColors {
    /// Unplayed track.
    pub track: Hsla,
    /// Played fill.
    pub fill: Hsla,
    /// Knob.
    pub knob: Hsla,
    /// Whether the knob casts a small shadow (on fields).
    pub knob_shadow: bool,
}

impl ScrubberColors {
    /// Scrubber on the window (floating player).
    pub fn window(cx: &App) -> Self {
        let c = cx.theme().colors;
        Self {
            track: c.hairline,
            fill: c.ink_secondary,
            knob: c.ink,
            knob_shadow: false,
        }
    }

    /// Scrubber on a book field.
    pub fn field(book: &BookPalette, knob_shadow: bool) -> Self {
        Self {
            track: book.on_field_track,
            fill: book.on_field,
            knob: book.on_field,
            knob_shadow,
        }
    }
}

/// Knob shadow alpha on fields (`0 1px 4px` black 35%).
const KNOB_SHADOW: (f32, f32, f32) = (1.0, 4.0, 0.35);

/// A horizontal playback position bar with a round knob. Click or drag to seek.
#[derive(IntoElement)]
pub struct Scrubber {
    id: ElementId,
    fraction: f32,
    colors: ScrubberColors,
    track: f32,
    knob: f32,
    on_seek: Option<SeekHandler>,
}

impl Scrubber {
    /// A scrubber at `fraction` (clamped to 0..=1).
    pub fn new(id: impl Into<ElementId>, fraction: f32, colors: ScrubberColors) -> Self {
        Self {
            id: id.into(),
            fraction: fraction.clamp(0.0, 1.0),
            colors,
            track: size::SCRUBBER_TRACK,
            knob: size::SCRUBBER_KNOB,
            on_seek: None,
        }
    }

    /// Track thickness and knob diameter.
    pub fn thickness(mut self, track: f32, knob: f32) -> Self {
        self.track = track;
        self.knob = knob;
        self
    }

    /// Sets the seek handler.
    pub fn on_seek(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_seek = Some(Rc::new(handler));
        self
    }
}

fn fraction_at(bounds: Bounds<Pixels>, x: Pixels) -> f32 {
    let width = f32::from(bounds.size.width);
    if width <= 0.0 {
        return 0.0;
    }
    (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0)
}

impl RenderOnce for Scrubber {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let c = self.colors;
        let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
        let knob_shadow = c.knob_shadow.then(|| {
            vec![
                BoxShadow::new(
                    px(0.0),
                    px(KNOB_SHADOW.0),
                    with_alpha(hex(mono::BLACK), KNOB_SHADOW.2),
                )
                .blur_radius(px(KNOB_SHADOW.1)),
            ]
        });

        div()
            .id(self.id)
            .relative()
            .w_full()
            .h(px(self.knob))
            .flex()
            .items_center()
            .child({
                let bounds = bounds.clone();
                canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full()
            })
            .child(
                div()
                    .w_full()
                    .h(px(self.track))
                    .rounded_full()
                    .bg(c.track)
                    .child(
                        div()
                            .h_full()
                            .w(relative(self.fraction))
                            .rounded_full()
                            .bg(c.fill),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(relative(self.fraction))
                    .ml(px(-self.knob / 2.0))
                    .size(px(self.knob))
                    .rounded_full()
                    .bg(c.knob)
                    .when_some(knob_shadow, |el, shadow| el.shadow(shadow)),
            )
            .when_some(self.on_seek, |el, handler| {
                let down = handler.clone();
                let down_bounds = bounds.clone();
                el.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    if let Some(b) = down_bounds.get() {
                        down(fraction_at(b, event.position.x), window, cx);
                    }
                })
                .on_mouse_move(move |event, window, cx| {
                    if event.pressed_button == Some(MouseButton::Left)
                        && let Some(b) = bounds.get()
                    {
                        handler(fraction_at(b, event.position.x), window, cx);
                    }
                })
            })
    }
}

/// Chapter-segmented progress: finished chapters, the current chapter (with its
/// own fill), and the chapters still to come, separated by small gaps.
///
/// The three weights are flex-grow factors, so `done` and `rest` are chapter
/// counts and the current chapter counts as one.
#[derive(IntoElement)]
pub struct SegmentedProgress {
    done: f32,
    current: f32,
    rest: f32,
    fill: Option<Hsla>,
    track: Option<Hsla>,
}

impl SegmentedProgress {
    /// `done` and `rest` chapter counts, and the fraction of the current chapter played.
    pub fn new(done: f32, current: f32, rest: f32) -> Self {
        Self {
            done: done.max(0.0),
            current: current.clamp(0.0, 1.0),
            rest: rest.max(0.0),
            fill: None,
            track: None,
        }
    }

    /// Overrides the fill and track colours (accent and hairline by default).
    pub fn colors(mut self, fill: Hsla, track: Hsla) -> Self {
        self.fill = Some(fill);
        self.track = Some(track);
        self
    }
}

fn segment(weight: f32) -> gpui_kit::Div {
    let mut el = div()
        .h_full()
        .rounded_full()
        .flex_basis(px(0.0))
        .min_w(px(0.0));
    el.style().flex_grow = Some(weight);
    el
}

impl RenderOnce for SegmentedProgress {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let fill = self.fill.unwrap_or(theme.accent.fill);
        let track = self.track.unwrap_or(theme.colors.hairline);
        div()
            .flex()
            .w_full()
            .h(px(size::SEGMENTS))
            .gap(px(size::SEGMENT_GAP))
            .when(self.done > 0.0, |el| el.child(segment(self.done).bg(fill)))
            .child(
                // GPUI clips to rectangles, so the fill rounds its own leading
                // corners instead of relying on the segment's overflow clip.
                segment(1.0).bg(track).child(
                    div()
                        .h_full()
                        .w(relative(self.current))
                        .rounded_l_full()
                        .bg(fill),
                ),
            )
            .when(self.rest > 0.0, |el| el.child(segment(self.rest).bg(track)))
    }
}
