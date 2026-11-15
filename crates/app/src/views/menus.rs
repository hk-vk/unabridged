//! Dropdown menus for the playback controls: sleep timer, speed and chapters.
use crate::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use ui::components::MenuTrigger;

/// Sleep timer choices, in menu order.
pub(crate) const SLEEP_CHOICES: [&str; 6] = [
    "Off",
    "15 minutes",
    "30 minutes",
    "45 minutes",
    "1 hour",
    "End of chapter",
];

/// Playback speeds offered in the menu.
pub(crate) const SPEEDS: [f32; 11] = [0.75, 0.9, 1.0, 1.1, 1.2, 1.3, 1.5, 1.75, 2.0, 2.5, 3.0];

impl Shell {
    /// Opens the sleep timer choices from `control`.
    pub(crate) fn sleep_menu(
        &self,
        id: &'static str,
        anchor: gpui_kit::Anchor,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let current = if matches!(self.sleep, SleepTimer::Off) {
            "Off"
        } else {
            self.sleep_choice
        };
        let has_chapters = self
            .player
            .as_ref()
            .is_some_and(|active| !active.chapters.is_empty());
        MenuTrigger::new(id, control).dropdown_menu_with_anchor(anchor, move |menu, _, _| {
            let mut menu = menu.min_w(px(180.)).label("Stop playing after");
            for choice in SLEEP_CHOICES {
                let weak = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(choice)
                        .checked(choice == current)
                        .disabled(choice == "End of chapter" && !has_chapters)
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |shell, cx| shell.set_sleep(choice, cx));
                        }),
                );
            }
            menu
        })
    }

    /// Opens the playback speeds from `control`.
    pub(crate) fn speed_menu(
        &self,
        id: &'static str,
        anchor: gpui_kit::Anchor,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let current = self.player.as_ref().map(|active| active.player.rate());
        MenuTrigger::new(id, control).dropdown_menu_with_anchor(anchor, move |menu, _, _| {
            let mut menu = menu.min_w(px(140.)).label("Speed");
            for speed in SPEEDS {
                let weak = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(model::format::speed(speed))
                        .checked(current.is_some_and(|rate| (rate - speed).abs() < 0.01))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |shell, cx| shell.set_speed(speed, cx));
                        }),
                );
            }
            menu
        })
    }

    /// Opens the chapter list from `control`, with the playing chapter checked.
    pub(crate) fn chapter_menu(
        &self,
        id: &'static str,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let (chapters, current, duration) = match &self.player {
            Some(active) => (
                active
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(index, chapter)| {
                        let name = if chapter.title.is_empty() {
                            format!("Chapter {}", index + 1)
                        } else {
                            chapter.title.clone()
                        };
                        (name, chapter.start_offset_ms as f64 / 1000.)
                    })
                    .collect::<Vec<_>>(),
                active.timeline().chapter.map(|(index, _)| index),
                active.player.duration().max(1.),
            ),
            None => (Vec::new(), None, 1.),
        };
        MenuTrigger::new(id, control).dropdown_menu_with_anchor(
            gpui_kit::Anchor::TopCenter,
            move |menu, _, _| {
                let mut menu = menu.min_w(px(260.)).max_h(px(420.)).scrollable(true);
                for (index, (name, start)) in chapters.iter().enumerate() {
                    let weak = weak.clone();
                    let target = (start / duration).clamp(0., 1.) as f32;
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(current == Some(index))
                            .on_click(move |_, _, cx| {
                                let _ =
                                    weak.update(cx, |shell, cx| shell.seek_fraction(target, cx));
                            }),
                    );
                }
                menu
            },
        )
    }
}
