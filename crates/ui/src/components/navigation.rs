//! Window chrome pieces: sidebar rows and headers, the toolbar, section headers.

use std::rc::Rc;

use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::button::ClickHandler;
use crate::icon::{Icon, icon};
use crate::metrics::{icon as icon_size, radius, size};
use crate::theme::ActiveTheme;
use crate::typography::{TypographyExt, text};

/// Horizontal padding inside sidebar rows and headers.
pub const SIDEBAR_ROW_PADDING: f32 = 10.0;
/// Gap between a sidebar glyph and its label.
const SIDEBAR_ROW_GAP: f32 = 8.0;
/// Bottom padding under a sidebar group header.
const SIDEBAR_HEADER_BOTTOM: f32 = 4.0;
/// Horizontal padding of the toolbar.
pub const TOOLBAR_PADDING: f32 = 28.0;
/// Gap between toolbar items.
const TOOLBAR_GAP: f32 = 14.0;

/// One row in the sidebar: glyph and label, accent tint when selected.
#[derive(IntoElement)]
pub struct SidebarItem {
    id: ElementId,
    icon: Icon,
    label: SharedString,
    selected: bool,
    trailing: Option<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl SidebarItem {
    /// A row with `icon` and `label`.
    pub fn new(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            selected: false,
            trailing: None,
            on_click: None,
        }
    }

    /// Marks the row as the current destination.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Puts a small accessory (a status dot, a count) at the row's end.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
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

impl RenderOnce for SidebarItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (label_color, style) = if self.selected {
            (theme.accent.on_tint, text::BODY_MEDIUM)
        } else {
            (theme.colors.ink, text::BODY)
        };
        div()
            .id(self.id)
            .flex()
            .items_center()
            .h(px(size::SIDEBAR_ROW))
            .px(px(SIDEBAR_ROW_PADDING))
            .gap(px(SIDEBAR_ROW_GAP))
            .rounded(px(radius::ROW))
            .when(self.selected, |el| el.bg(theme.accent.tint))
            .child(
                icon(self.icon)
                    .flex_none()
                    .size(px(icon_size::MEDIUM))
                    .text_color(theme.accent.text),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(label_color)
                    .type_style(style)
                    .child(self.label),
            )
            .children(self.trailing)
            .when_some(self.on_click, |el, handler| {
                // The selected row already has its tint; other rows show the
                // window fill under the pointer so the hover is visible.
                let selected = self.selected;
                let fill = theme.colors.fill;
                el.cursor_pointer()
                    .hover(move |style| if selected { style.opacity(0.86) } else { style.bg(fill) })
                    .on_click(move |e, w, cx| handler(e, w, cx))
            })
    }
}

/// A sidebar group header ("Your library", "Browse"). The caller sets top spacing.
#[derive(IntoElement)]
pub struct SidebarHeader(SharedString);

impl SidebarHeader {
    /// A header reading `label`.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self(label.into())
    }
}

impl RenderOnce for SidebarHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .px(px(SIDEBAR_ROW_PADDING))
            .pb(px(SIDEBAR_HEADER_BOTTOM))
            .text_color(cx.theme().colors.ink_secondary)
            .type_style(text::FOOTNOTE_STRONG)
            .child(self.0)
    }
}

/// The content toolbar: page title on the left, controls on the right.
#[derive(IntoElement)]
pub struct Toolbar {
    title: SharedString,
    trailing: Vec<AnyElement>,
}

impl Toolbar {
    /// A toolbar titled `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            trailing: Vec::new(),
        }
    }

    /// Appends a trailing control.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }
}

impl RenderOnce for Toolbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(size::TOOLBAR))
            .px(px(TOOLBAR_PADDING))
            .gap(px(TOOLBAR_GAP))
            .child(
                div()
                    .text_color(cx.theme().colors.ink)
                    .type_style(text::PAGE_TITLE)
                    .child(self.title),
            )
            .child(div().flex_1())
            .children(self.trailing)
    }
}

/// A section heading with an optional trailing link ("See all").
#[derive(IntoElement)]
pub struct SectionHeader {
    id: ElementId,
    title: SharedString,
    action: Option<(SharedString, ClickHandler)>,
}

impl SectionHeader {
    /// A heading reading `title`.
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            action: None,
        }
    }

    /// Adds a trailing link.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .justify_between()
            .items_baseline()
            .child(
                div()
                    .text_color(theme.colors.ink)
                    .type_style(text::SECTION_TITLE)
                    .child(self.title),
            )
            .when_some(self.action, |el, (label, handler)| {
                el.child(
                    div()
                        .id(self.id)
                        .text_color(theme.accent.text)
                        .type_style(text::BODY)
                        .child(label)
                        .on_click(move |e, w, cx| handler(e, w, cx)),
                )
            })
    }
}
