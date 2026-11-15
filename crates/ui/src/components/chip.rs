//! Small labelled controls: speed chip, credits chip, sort popup, field pills.

use std::rc::Rc;

use gpui_kit::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::button::{ClickHandler, PRESSED_OPACITY};
use crate::elevation::{Elevation, ElevationExt};
use crate::icon::{Icon, icon};
use crate::metrics::{icon as icon_size, radius, size};
use crate::theme::ActiveTheme;
use crate::typography::{TypeStyle, TypographyExt, text};

/// Horizontal padding of a raised chip.
const RAISED_PADDING: f32 = 10.0;
/// Horizontal padding of a raised chip at the smallest height.
const RAISED_PADDING_SMALL: f32 = 9.0;
/// Horizontal padding of a filled popup.
const FILLED_PADDING: f32 = 8.0;
/// Horizontal padding of an outlined field pill.
const OUTLINE_PADDING: f32 = 12.0;
/// Gap between glyph and label.
const GAP: f32 = 6.0;
/// Gap between glyph and label in field pills.
const OUTLINE_GAP: f32 = 7.0;

/// Treatment of a [`Chip`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChipStyle {
    /// Raised neutral chip (speed, credits).
    Raised,
    /// Flat chip on the subtle fill (toolbar sort popup).
    Filled,
    /// Flat tinted chip, such as a toggle on a book field: `fill` behind `content`.
    Tint {
        /// Background colour.
        fill: Hsla,
        /// Text and glyph colour.
        content: Hsla,
    },
    /// Outlined pill on a book field: `outline` stroke, `content` text.
    Outline {
        /// Stroke colour.
        outline: Hsla,
        /// Text and glyph colour.
        content: Hsla,
    },
}

/// A compact control with an optional glyph and a label.
#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<Icon>,
    icon_color: Option<Hsla>,
    style: ChipStyle,
    height: f32,
    text: Option<TypeStyle>,
    on_click: Option<ClickHandler>,
}

impl Chip {
    /// A raised chip at the standard chip height.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            icon_color: None,
            style: ChipStyle::Raised,
            height: size::CHIP,
            text: None,
            on_click: None,
        }
    }

    /// Sets the label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the glyph and its colour.
    pub fn icon(mut self, icon: Icon, color: Hsla) -> Self {
        self.icon = Some(icon);
        self.icon_color = Some(color);
        self
    }

    /// Sets the treatment.
    pub fn style(mut self, style: ChipStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the height (see [`crate::metrics::size`]).
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Overrides the label type style.
    pub fn text(mut self, style: TypeStyle) -> Self {
        self.text = Some(style);
        self
    }

    /// Sets the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let base = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .h(px(self.height))
            .active(|style| style.opacity(PRESSED_OPACITY));

        let (el, content, glyph, default_text) = match self.style {
            ChipStyle::Raised => (
                base.rounded(px(radius::CHIP))
                    .px(px(if self.height <= size::CHIP {
                        RAISED_PADDING_SMALL
                    } else {
                        RAISED_PADDING
                    }))
                    .gap(px(GAP))
                    .elevation(Elevation::Chip, theme, window),
                theme.colors.ink,
                icon_size::SMALL,
                text::CAPTION_MEDIUM,
            ),
            ChipStyle::Filled => (
                base.rounded(px(radius::POPUP))
                    .px(px(FILLED_PADDING))
                    .gap(px(GAP))
                    .bg(theme.colors.fill),
                theme.colors.ink,
                icon_size::XSMALL,
                text::CAPTION,
            ),
            ChipStyle::Tint { fill, content } => (
                base.rounded(px(radius::POPUP))
                    .px(px(RAISED_PADDING))
                    .gap(px(GAP))
                    .bg(fill),
                content,
                icon_size::MEDIUM,
                text::CAPTION_MEDIUM,
            ),
            ChipStyle::Outline { outline, content } => (
                base.rounded_full()
                    .px(px(OUTLINE_PADDING))
                    .gap(px(OUTLINE_GAP))
                    .border_1()
                    .border_color(outline),
                content,
                icon_size::SMALL,
                text::BODY,
            ),
        };

        el.text_color(content)
            .type_style(self.text.unwrap_or(default_text))
            .when_some(self.icon, |el, glyph_icon| {
                el.child(
                    icon(glyph_icon)
                        .flex_none()
                        .size(px(glyph))
                        .text_color(self.icon_color.unwrap_or(content)),
                )
            })
            .when_some(self.label, |el, label| el.child(label))
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .hover(|style| style.opacity(0.86))
                    .on_click(move |e, w, cx| handler(e, w, cx))
            })
    }
}

/// A keyboard shortcut hint, such as `⌘F` in the search field.
#[derive(IntoElement)]
pub struct KeyHint(SharedString);

impl KeyHint {
    /// A hint showing `keys`.
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Self(keys.into())
    }
}

/// Horizontal padding of a key hint.
const KEY_HINT_PADDING: f32 = 6.0;

impl RenderOnce for KeyHint {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors;
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(size::KEY_HINT))
            .px(px(KEY_HINT_PADDING))
            .rounded(px(radius::KEY_HINT))
            .bg(colors.fill)
            .text_color(colors.ink_secondary)
            .type_style(text::KEY_HINT)
            .child(self.0)
    }
}
