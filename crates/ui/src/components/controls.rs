//! Form controls: switch, search field, grouped list rows.

use std::rc::Rc;

use gpui_kit::{
    AnyElement, App, ElementId, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window,
    component::input::{Input, InputState},
    div,
    prelude::FluentBuilder as _,
    px,
};

use super::button::{ClickHandler, PRESSED_OPACITY};
use super::chip::KeyHint;
use crate::elevation::{Elevation, ElevationExt};
use crate::icon::{Icon, icon};
use crate::metrics::{concentric, icon as icon_size, radius, size};
use crate::theme::ActiveTheme;
use crate::typography::{TypographyExt, text};

/// Inset of the knob inside the switch track.
const SWITCH_INSET: f32 = 2.0;

/// Toggle handler: receives the new state.
pub type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// An on/off switch.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    /// A switch showing `on`.
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            on_toggle: None,
        }
    }

    /// Sets the toggle handler.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (width, height) = size::SWITCH;
        let track = if self.on {
            Elevation::SwitchOn
        } else {
            Elevation::SwitchOff
        };
        let on = self.on;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .w(px(width))
            .h(px(height))
            .px(px(SWITCH_INSET))
            .rounded_full()
            .when(on, |el| el.justify_end())
            .elevation(track, theme, window)
            .child(div().size(px(size::SWITCH_KNOB)).rounded_full().elevation(
                Elevation::Knob,
                theme,
                window,
            ))
            .when_some(self.on_toggle, |el, handler| {
                el.on_click(move |_, window, cx| handler(!on, window, cx))
            })
    }
}

/// Horizontal padding inside the search field.
const SEARCH_PADDING: f32 = 11.0;
/// Gap between the search glyph, text and key hint.
const SEARCH_GAP: f32 = 8.0;

/// The toolbar search field: glyph, text input and a key hint.
///
/// The text editing itself is gpui-component's [`Input`] with its own chrome
/// turned off; this component supplies the recessed field around it.
#[derive(IntoElement)]
pub struct SearchField {
    input: Entity<InputState>,
    shortcut: SharedString,
}

impl SearchField {
    /// A field editing `input`, showing `shortcut` as its key hint.
    pub fn new(input: &Entity<InputState>, shortcut: impl Into<SharedString>) -> Self {
        Self {
            input: input.clone(),
            shortcut: shortcut.into(),
        }
    }
}

impl RenderOnce for SearchField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let empty = self.input.read(cx).value().is_empty();
        div()
            .flex()
            .flex_none()
            .items_center()
            .w(px(size::SEARCH_WIDTH))
            .h(px(size::SEARCH_HEIGHT))
            .px(px(SEARCH_PADDING))
            .gap(px(SEARCH_GAP))
            .rounded(px(radius::FIELD))
            .elevation(Elevation::Field, theme, window)
            .child(
                icon(Icon::Search)
                    .flex_none()
                    .size(px(icon_size::MEDIUM))
                    .text_color(theme.colors.ink_secondary),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.input)
                        .appearance(false)
                        .text_color(theme.colors.ink)
                        .type_style(text::BODY),
                ),
            )
            .when(empty, |el| el.child(KeyHint::new(self.shortcut)))
    }
}

/// A plain one-line text field, as wide as its container.
#[derive(IntoElement)]
pub struct TextField {
    input: Entity<InputState>,
}

impl TextField {
    /// A field editing `input`.
    pub fn new(input: &Entity<InputState>) -> Self {
        Self {
            input: input.clone(),
        }
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(size::SEARCH_HEIGHT))
            .px(px(SEARCH_PADDING))
            .rounded(px(radius::FIELD))
            .elevation(Elevation::Field, theme, window)
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.input)
                        .appearance(false)
                        .text_color(theme.colors.ink)
                        .type_style(text::BODY),
                ),
            )
    }
}

/// Horizontal padding of a grouped list row.
const ROW_PADDING: f32 = 16.0;
/// Gap between a row's leading element, text and trailing element.
const ROW_GAP: f32 = 16.0;
/// Grouped list row height.
const ROW_HEIGHT: f32 = 56.0;
/// Border width of a grouped list card.
const GROUP_BORDER: f32 = 1.0;

/// A row in a [`GroupedList`]: leading element, title, subtitle, trailing element.
#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    leading: Option<AnyElement>,
    title: SharedString,
    subtitle: Option<SharedString>,
    trailing: Option<AnyElement>,
    selected: bool,
    /// Whether the row is first / last in its group (rounds those corners).
    edges: (bool, bool),
    on_click: Option<ClickHandler>,
}

impl ListRow {
    /// A row titled `title`.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            leading: None,
            title: title.into(),
            subtitle: None,
            trailing: None,
            selected: false,
            edges: (false, false),
            on_click: None,
        }
    }

    /// Sets the leading element.
    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    /// Sets the subtitle.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets the trailing element.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
        self
    }

    /// Highlights the row with the accent tint.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        // GPUI clips to rectangles, so edge rows round their own corners to sit
        // concentrically inside the card's 1 pt border.
        let corner = px(concentric(radius::CARD, GROUP_BORDER));
        let (first, last) = self.edges;
        div()
            .id(self.id)
            .flex()
            .items_center()
            .h(px(ROW_HEIGHT))
            .when(first, |el| el.rounded_t(corner))
            .when(last, |el| el.rounded_b(corner))
            .px(px(ROW_PADDING))
            .gap(px(ROW_GAP))
            .when(self.selected, |el| el.bg(theme.accent.tint))
            .children(self.leading)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(theme.colors.ink)
                            .type_style(text::LABEL_STRONG)
                            .child(self.title),
                    )
                    .when_some(self.subtitle, |el, subtitle| {
                        el.child(
                            div()
                                .text_color(theme.colors.ink_secondary)
                                .type_style(text::CAPTION)
                                .child(subtitle),
                        )
                    }),
            )
            .children(self.trailing)
            .when_some(self.on_click, |el, handler| {
                el.active(|style| style.opacity(PRESSED_OPACITY))
                    .on_click(move |e, w, cx| handler(e, w, cx))
            })
    }
}

/// Rows grouped in a rounded, hairline-bordered card with dividers between them.
#[derive(IntoElement, Default)]
pub struct GroupedList {
    rows: Vec<ListRow>,
}

impl GroupedList {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a row.
    pub fn row(mut self, row: ListRow) -> Self {
        self.rows.push(row);
        self
    }
}

impl RenderOnce for GroupedList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hairline = cx.theme().colors.hairline;
        let count = self.rows.len();
        div()
            .flex()
            .flex_col()
            .rounded(px(radius::CARD))
            .border_1()
            .border_color(hairline)
            .children(self.rows.into_iter().enumerate().map(|(ix, mut row)| {
                row.edges = (ix == 0, ix + 1 == count);
                div()
                    .when(ix + 1 < count, |el| el.border_b_1().border_color(hairline))
                    .child(row)
            }))
    }
}
