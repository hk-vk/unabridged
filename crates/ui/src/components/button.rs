//! Push buttons and plain glyph buttons.

use std::rc::Rc;

use gpui_kit::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
    relative,
};

use crate::elevation::{Elevation, ElevationExt};
use crate::icon::{Icon, icon};
use crate::metrics::{radius, size};
use crate::theme::{ActiveTheme, BookPalette};
use crate::typography::{TypeStyle, TypographyExt, text};

/// Click handler shared by the interactive components.
pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Opacity of a control while pressed.
pub(crate) const PRESSED_OPACITY: f32 = 0.82;

/// Horizontal padding and icon gap of a labelled regular button.
const REGULAR_PADDING: (f32, f32) = (16.0, 7.0);
/// Primary buttons are a touch wider, as in the design.
const PRIMARY_PADDING: f32 = 18.0;
/// Horizontal padding and icon gap of a toolbar button.
const TOOLBAR_PADDING: (f32, f32) = (11.0, 6.0);
/// Glyph size inside regular buttons.
const REGULAR_ICON: f32 = 12.0;
/// Glyph size inside toolbar buttons.
const TOOLBAR_ICON: f32 = 14.0;

/// Visual treatment of a [`Button`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonVariant {
    /// Accent gradient, for the one main action in a view.
    Primary,
    /// Neutral raised button.
    Secondary,
    /// Ink (dark) button; round play/pause on the window.
    Ink,
    /// Light button on a book field.
    Light(BookPalette),
    /// Light button on a book field, large drop (72 pt play).
    LightLarge(BookPalette),
    /// Translucent button on a book field.
    Glass(BookPalette),
}

impl ButtonVariant {
    fn elevation(self) -> Elevation {
        match self {
            ButtonVariant::Primary => Elevation::ButtonPrimary,
            ButtonVariant::Secondary => Elevation::ButtonSecondary,
            ButtonVariant::Ink => Elevation::ButtonInk,
            ButtonVariant::Light(book) => Elevation::ButtonLight(book),
            ButtonVariant::LightLarge(book) => Elevation::ButtonLightLarge(book),
            ButtonVariant::Glass(book) => Elevation::ButtonGlass(book),
        }
    }

    fn label_style(self) -> TypeStyle {
        match self {
            ButtonVariant::Primary | ButtonVariant::Light(_) | ButtonVariant::LightLarge(_) => {
                text::BODY_STRONG
            }
            _ => text::BODY_MEDIUM,
        }
    }
}

/// Size and shape of a [`Button`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonShape {
    /// 36 pt rounded rectangle.
    Regular,
    /// 30 pt toolbar rounded rectangle.
    Toolbar,
    /// Circle of the given diameter; `icon` is the glyph size.
    Circle {
        /// Diameter in points.
        diameter: f32,
        /// Glyph size in points.
        icon: f32,
    },
}

/// A push button: rounded rectangle or circle, with a label and/or a glyph.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<Icon>,
    icon_color: Option<Hsla>,
    variant: ButtonVariant,
    shape: ButtonShape,
    width: Option<f32>,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// A secondary regular button.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            icon_color: None,
            variant: ButtonVariant::Secondary,
            shape: ButtonShape::Regular,
            width: None,
            on_click: None,
        }
    }

    /// Sets the label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the leading glyph.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Tints the glyph differently from the label (e.g. an accent glyph).
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// Sets the variant.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the shape.
    pub fn shape(mut self, shape: ButtonShape) -> Self {
        self.shape = shape;
        self
    }

    /// Fixes the width (labels centre).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
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

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let content = match self.variant {
            ButtonVariant::Primary => theme.accent.on_fill,
            ButtonVariant::Secondary => theme.colors.ink,
            ButtonVariant::Ink => theme.colors.on_ink,
            ButtonVariant::Light(book) | ButtonVariant::LightLarge(book) => book.field,
            ButtonVariant::Glass(book) => book.on_field,
        };
        let glyph_color = self.icon_color.unwrap_or(content);

        let base = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .text_color(content)
            .elevation(self.variant.elevation(), theme, window)
            .active(|style| style.opacity(PRESSED_OPACITY));

        let (base, glyph_size) = match self.shape {
            ButtonShape::Regular | ButtonShape::Toolbar => {
                let toolbar = self.shape == ButtonShape::Toolbar;
                let (padding, gap) = if toolbar {
                    TOOLBAR_PADDING
                } else if self.variant == ButtonVariant::Primary {
                    (PRIMARY_PADDING, REGULAR_PADDING.1)
                } else {
                    REGULAR_PADDING
                };
                let style = if toolbar {
                    text::CAPTION_MEDIUM
                } else {
                    self.variant.label_style()
                };
                let el = base
                    .h(px(if toolbar {
                        size::TOOLBAR_BUTTON
                    } else {
                        size::BUTTON
                    }))
                    .rounded(px(if toolbar {
                        radius::CHIP
                    } else {
                        radius::BUTTON
                    }))
                    .px(px(padding))
                    .gap(px(gap))
                    .type_style(style);
                (el, if toolbar { TOOLBAR_ICON } else { REGULAR_ICON })
            }
            ButtonShape::Circle { diameter, icon } => {
                (base.size(px(diameter)).rounded_full(), icon)
            }
        };

        base.when_some(self.width, |el, width| el.w(px(width)))
            .when_some(self.icon, |el, glyph| {
                el.child(
                    icon(glyph)
                        .flex_none()
                        .size(px(glyph_size))
                        .text_color(glyph_color),
                )
            })
            .when_some(self.label, |el, label| {
                el.child(div().line_height(relative(1.0)).child(label))
            })
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .hover(|style| style.opacity(0.86))
                    .on_click(move |e, w, cx| handler(e, w, cx))
            })
    }
}

/// A bare glyph button (transport skips, utilities): no chrome, pressed dims.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    size: f32,
    color: Option<Hsla>,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    /// A glyph button drawing `icon` at `size` points.
    pub fn new(id: impl Into<ElementId>, icon: Icon, size: f32) -> Self {
        Self {
            id: id.into(),
            icon,
            size,
            color: None,
            on_click: None,
        }
    }

    /// Glyph colour; defaults to secondary ink.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
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

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().colors.ink_secondary);
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(self.size))
            .active(|style| style.opacity(PRESSED_OPACITY))
            .child(icon(self.icon).size(px(self.size)).text_color(color))
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .hover(|style| style.opacity(0.86))
                    .on_click(move |e, w, cx| handler(e, w, cx))
            })
    }
}
