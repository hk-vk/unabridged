//! Progress displays: the seekable scrubber and chapter-segmented progress.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    App, Bounds, BoxShadow, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
    canvas, div, prelude::FluentBuilder as _, px, relative,
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
    hit: f32,
    on_seek: Option<SeekHandler>,
    sections: Vec<(f32, SharedString)>,
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
            hit: 0.0,
            on_seek: None,
            sections: Vec::new(),
        }
    }

    /// Splits the track into named segments, one per `(start, name)` with
    /// the start as a fraction of the bar (such as a chapter's sections),
    /// with a small gap between them. Hovering a segment shows its name.
    pub fn segments(mut self, sections: impl IntoIterator<Item = (f32, SharedString)>) -> Self {
        let mut sections: Vec<_> = sections
            .into_iter()
            .filter(|(at, _)| (0.0..1.0).contains(at))
            .collect();
        sections.sort_by(|a, b| a.0.total_cmp(&b.0));
        sections.dedup_by(|a, b| a.0 == b.0);
        // One segment is just the plain bar.
        if sections.len() > 1 {
            if let Some(first) = sections.first_mut() {
                first.0 = 0.0;
            }
            self.sections = sections;
        }
        self
    }

    /// Track thickness and knob diameter.
    pub fn thickness(mut self, track: f32, knob: f32) -> Self {
        self.track = track;
        self.knob = knob;
        self
    }

    /// Makes the clickable strip at least `height` tall, centred on the track.
    pub fn hit_area(mut self, height: f32) -> Self {
        self.hit = height;
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

        let height = self.hit.max(self.knob);
        div()
            .id(self.id)
            .relative()
            .w_full()
            .h(px(height))
            .flex()
            .items_center()
            .child({
                let bounds = bounds.clone();
                canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .size_full()
            })
            .when(self.sections.is_empty(), |el| {
                el.child(
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
            })
            .children({
                // Each segment sits at its exact place on the bar, inset by
                // half a gap on inner edges, so the fill ends under the knob.
                // Its hover area is the scrubber's full height, not the thin
                // track, and names the segment.
                let ends: Vec<f32> = self
                    .sections
                    .iter()
                    .skip(1)
                    .map(|(at, _)| *at)
                    .chain([1.0])
                    .collect();
                let last = self.sections.len().saturating_sub(1);
                let half = size::SEGMENT_GAP / 2.0;
                let (fraction, track) = (self.fraction, self.track);
                self.sections.into_iter().zip(ends).enumerate().map(
                    move |(index, ((from, name), to))| {
                        let filled = ((fraction - from) / (to - from)).clamp(0.0, 1.0);
                        let group = SharedString::from(format!("scrubber-segment-{index}"));
                        div()
                            .id(("scrubber-segment", index))
                            .group(group.clone())
                            .absolute()
                            .top_0()
                            .h(px(height))
                            .left(relative(from))
                            .w(relative(to - from))
                            .flex()
                            .items_center()
                            .when(index > 0, |el| el.pl(px(half)))
                            .when(index < last, |el| el.pr(px(half)))
                            .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
                            .child(
                                div()
                                    .w_full()
                                    .h(px(track))
                                    .rounded_full()
                                    .bg(c.track)
                                    .group_hover(group, |style| style.bg(c.fill.opacity(0.4)))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(filled))
                                            .rounded_full()
                                            .bg(c.fill),
                                    ),
                            )
                    },
                )
            })
            .child(
                div()
                    .absolute()
                    .top(px((height - self.knob) / 2.0))
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
