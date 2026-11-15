//! Opening a dropdown menu from any of our controls (a chip, an icon button).

use gpui_kit::{
    Div, InteractiveElement, Interactivity, IntoElement, ParentElement, RenderOnce, Stateful,
    StyleRefinement, Styled, Window, component::menu::DropdownMenu, div,
};

/// Wraps a control so `.dropdown_menu(…)` can open a menu from it. Clicks on
/// the wrapped control open the menu, so give the control no click handler.
#[derive(IntoElement)]
pub struct MenuTrigger {
    base: Stateful<Div>,
    selected: bool,
}

impl MenuTrigger {
    /// A trigger around `control`.
    pub fn new(id: impl Into<gpui_kit::ElementId>, control: impl IntoElement) -> Self {
        Self {
            base: div().id(id).child(control),
            selected: false,
        }
    }
}

impl Styled for MenuTrigger {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for MenuTrigger {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl gpui_kit::base::Selectable for MenuTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for MenuTrigger {
    fn render(self, _window: &mut Window, _cx: &mut gpui_kit::App) -> impl IntoElement {
        self.base
    }
}

impl DropdownMenu for MenuTrigger {}
