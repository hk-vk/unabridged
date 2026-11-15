//! Pickers for the playback controls (chapters, sleep timer, speed): one
//! popover style with radio rows, in the colours of the surface it opens on.
use crate::*;
use gpui_kit::component::popover::Popover;
use std::rc::Rc;
use ui::components::MenuTrigger;

/// Sleep timer choices, in order.
pub(crate) const SLEEP_CHOICES: [&str; 6] = [
    "Off",
    "15 minutes",
    "30 minutes",
    "45 minutes",
    "1 hour",
    "End of chapter",
];

/// Playback speed presets.
pub(crate) const SPEEDS: [f32; 8] = [0.75, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5];
/// Fine speed step for the − and + buttons.
const SPEED_STEP: f32 = 0.05;

/// Colours of a picker: the player's book field, or the window.
#[derive(Clone, Copy)]
pub(crate) struct PickerColors {
    surface: gpui_kit::Hsla,
    ink: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
    selected: gpui_kit::Hsla,
    hairline: gpui_kit::Hsla,
}

impl PickerColors {
    pub(crate) fn field(palette: &ui::theme::BookPalette) -> Self {
        Self {
            surface: palette.field_deep,
            ink: palette.on_field,
            muted: palette.on_field_muted,
            selected: palette.on_field_track,
            hairline: palette.on_field_track,
        }
    }

    pub(crate) fn window(cx: &gpui_kit::App) -> Self {
        let colors = cx.theme().colors;
        Self {
            surface: colors.window,
            ink: colors.ink,
            muted: colors.ink_secondary,
            selected: colors.fill,
            hairline: colors.hairline,
        }
    }
}

/// What picking a row does to the shell.
type Pick = Rc<dyn Fn(&mut Shell, &mut Context<Shell>)>;

/// One row of a picker.
struct Choice {
    label: String,
    detail: Option<String>,
    selected: bool,
    pick: Pick,
}

type Choices = Rc<dyn Fn(&Shell) -> Vec<Choice>>;
type Header = Rc<dyn Fn(&Shell, PickerColors, gpui_kit::WeakEntity<Shell>) -> gpui_kit::AnyElement>;

/// A radio dot: a ring, filled with a centre dot when selected.
fn radio(selected: bool, colors: PickerColors) -> gpui_kit::Div {
    div()
        .flex_none()
        .size(px(18.))
        .rounded_full()
        .border_1()
        .border_color(if selected {
            colors.ink
        } else {
            colors.muted.opacity(0.6)
        })
        .flex()
        .items_center()
        .justify_center()
        .when(selected, |dot| {
            dot.bg(colors.ink)
                .child(div().size(px(7.)).rounded_full().bg(colors.surface))
        })
}

/// A popover of radio rows opened from `trigger`. Rows are rebuilt from the
/// shell on every render, so the selection stays live; picking one closes it.
#[allow(clippy::too_many_arguments)]
fn picker(
    id: &'static str,
    anchor: gpui_kit::Anchor,
    width: f32,
    colors: PickerColors,
    trigger: impl IntoElement,
    header: Option<Header>,
    choices: Choices,
    scroll: Option<gpui_kit::ScrollHandle>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let shell = cx.entity().downgrade();
    let on_open_scroll = scroll.clone();
    let choices_for_open = choices.clone();
    let shell_for_open = shell.clone();
    Popover::new(id)
        .anchor(anchor)
        .trigger(MenuTrigger::new(format!("{id}-trigger"), trigger))
        .w(px(width))
        .p(px(6.))
        .rounded(px(14.))
        .bg(colors.surface)
        .border_1()
        .border_color(colors.hairline)
        .on_open_change(move |open, _, cx| {
            // Open on the selected row.
            if *open
                && let Some(handle) = &on_open_scroll
                && let Some(shell) = shell_for_open.upgrade()
                && let Some(index) = choices_for_open(shell.read(cx))
                    .iter()
                    .position(|choice| choice.selected)
            {
                handle.scroll_to_top_of_item(index.saturating_sub(2));
            }
        })
        .content(move |_, _, cx| {
            let Some(entity) = shell.upgrade() else {
                return div().into_any_element();
            };
            let popover = cx.entity();
            let rows = choices(entity.read(cx));
            let mut list = div()
                .id("picker-rows")
                .flex()
                .flex_col()
                .gap(px(2.))
                .max_h(px(380.))
                .overflow_y_scroll();
            if let Some(handle) = &scroll {
                list = list.track_scroll(handle);
            }
            for (index, choice) in rows.into_iter().enumerate() {
                let pick = choice.pick.clone();
                let weak = shell.clone();
                let popover = popover.clone();
                list = list.child(
                    div()
                        .id(("picker-row", index))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .min_h(px(36.))
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(9.))
                        .cursor_pointer()
                        .when(choice.selected, |row| row.bg(colors.selected))
                        .hover(move |style| style.bg(colors.selected))
                        .active(|style| style.opacity(0.8))
                        .on_click(move |_, window, cx| {
                            let _ = weak.update(cx, |shell, cx| pick(shell, cx));
                            popover.update(cx, |state, cx| state.dismiss(window, cx));
                        })
                        .child(radio(choice.selected, colors))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .type_style(if choice.selected {
                                    ui::text::BODY_MEDIUM
                                } else {
                                    ui::text::BODY
                                })
                                .text_color(colors.ink)
                                .child(choice.label),
                        )
                        .children(choice.detail.map(|detail| {
                            div()
                                .flex_none()
                                .type_style(ui::text::CAPTION.tabular())
                                .text_color(colors.muted)
                                .child(detail)
                        })),
                );
            }
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(
                    header
                        .as_ref()
                        .map(|header| header(entity.read(cx), colors, shell.clone())),
                )
                .child(list)
                .into_any_element()
        })
}

/// The local time `after` from now, like "11:45 PM".
fn wall_clock_in(after: std::time::Duration) -> String {
    let at = chrono::Local::now() + chrono::Duration::from_std(after).unwrap_or_default();
    at.format("%-I:%M %p").to_string()
}

/// A small caption above the rows.
fn title(text: &'static str, colors: PickerColors) -> gpui_kit::AnyElement {
    div()
        .px(px(10.))
        .pt(px(6.))
        .pb(px(4.))
        .type_style(ui::text::FOOTNOTE_STRONG)
        .text_color(colors.muted)
        .child(text)
        .into_any_element()
}

impl Shell {
    /// The chapter picker: every chapter, the playing one selected.
    pub(crate) fn chapter_picker(
        &self,
        id: &'static str,
        anchor: gpui_kit::Anchor,
        colors: PickerColors,
        trigger: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let choices: Choices = Rc::new(|shell: &Shell| {
            let Some(active) = &shell.player else {
                return Vec::new();
            };
            let current = active.timeline().chapter.map(|(index, _)| index);
            let duration = active.player.duration().max(1.);
            active
                .chapters
                .iter()
                .enumerate()
                .map(|(index, chapter)| {
                    let target =
                        (chapter.start_offset_ms as f64 / 1000. / duration).clamp(0., 1.) as f32;
                    Choice {
                        label: if chapter.title.is_empty() {
                            format!("Chapter {}", index + 1)
                        } else {
                            chapter.title.clone()
                        },
                        detail: Some(model::format::clock(std::time::Duration::from_millis(
                            chapter.length_ms,
                        ))),
                        selected: current == Some(index),
                        pick: Rc::new(move |shell, cx| shell.seek_fraction(target, cx)),
                    }
                })
                .collect()
        });
        picker(
            id,
            anchor,
            380.,
            colors,
            trigger,
            Some(Rc::new(|_, colors, _| title("Chapters", colors))),
            choices,
            Some(self.chapter_picker_scroll.clone()),
            cx,
        )
    }

    /// The sleep timer picker, with when each choice would stop playback.
    pub(crate) fn sleep_picker(
        &self,
        id: &'static str,
        anchor: gpui_kit::Anchor,
        colors: PickerColors,
        trigger: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let choices: Choices = Rc::new(|shell: &Shell| {
            let current = if matches!(shell.sleep, SleepTimer::Off) {
                "Off"
            } else {
                shell.sleep_choice
            };
            let chapter_left = shell
                .player
                .as_ref()
                .and_then(|active| {
                    active
                        .timeline()
                        .chapter
                        .as_ref()
                        .map(|_| active.timeline())
                })
                .map(|timeline| timeline.remaining());
            SLEEP_CHOICES
                .into_iter()
                .filter(|choice| *choice != "End of chapter" || chapter_left.is_some())
                .map(|choice| {
                    let minutes = match choice {
                        "15 minutes" => Some(15),
                        "30 minutes" => Some(30),
                        "45 minutes" => Some(45),
                        "1 hour" => Some(60),
                        _ => None,
                    };
                    let detail = match (choice, minutes) {
                        (_, Some(minutes)) => Some(format!(
                            "until {}",
                            wall_clock_in(std::time::Duration::from_secs(minutes * 60))
                        )),
                        ("End of chapter", _) => {
                            chapter_left.map(|left| format!("in {}", model::format::clock(left)))
                        }
                        _ => None,
                    };
                    Choice {
                        label: choice.to_string(),
                        detail,
                        selected: choice == current,
                        pick: Rc::new(move |shell, cx| shell.set_sleep(choice, cx)),
                    }
                })
                .collect()
        });
        picker(
            id,
            anchor,
            260.,
            colors,
            trigger,
            Some(Rc::new(|_, colors, _| title("Stop playing", colors))),
            choices,
            None,
            cx,
        )
    }

    /// The speed picker: the exact rate with − and + for fine steps, over
    /// common presets.
    pub(crate) fn speed_picker(
        &self,
        id: &'static str,
        anchor: gpui_kit::Anchor,
        colors: PickerColors,
        trigger: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let choices: Choices = Rc::new(|shell: &Shell| {
            let rate = shell.player.as_ref().map(|active| active.player.rate());
            SPEEDS
                .into_iter()
                .map(|speed| Choice {
                    label: model::format::speed(speed),
                    detail: (speed == 1.0).then(|| "Normal".to_string()),
                    selected: rate.is_some_and(|rate| (rate - speed).abs() < 0.01),
                    pick: Rc::new(move |shell, cx| shell.set_speed(speed, cx)),
                })
                .collect()
        });
        let header: Header = Rc::new(|shell: &Shell, colors, weak| {
            let rate = shell
                .player
                .as_ref()
                .map(|active| active.player.rate())
                .unwrap_or(1.);
            let step =
                |id: &'static str, icon: Icon, delta: f32, weak: gpui_kit::WeakEntity<Shell>| {
                    IconButton::new(id, icon, 14.)
                        .hit_area(32.)
                        .color(colors.ink)
                        .hover_fill(colors.selected)
                        .tooltip(if delta < 0. { "Slower" } else { "Faster" })
                        .on_click(move |_, _, cx| {
                            let next = ((rate + delta) * 100.).round() / 100.;
                            let _ = weak
                                .update(cx, |shell, cx| shell.set_speed(next.clamp(0.5, 3.0), cx));
                        })
                };
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(6.))
                .pt(px(6.))
                .pb(px(8.))
                .mb(px(2.))
                .border_b_1()
                .border_color(colors.hairline)
                .child(step("speed-slower", Icon::Minus, -SPEED_STEP, weak.clone()))
                .child(
                    div()
                        .font_family("Geist Pixel")
                        .text_size(px(30.))
                        .text_color(colors.ink)
                        .child(format!("{rate:.2}×")),
                )
                .child(step("speed-faster", Icon::Plus, SPEED_STEP, weak))
                .into_any_element()
        });
        picker(
            id,
            anchor,
            220.,
            colors,
            trigger,
            Some(header),
            choices,
            None,
            cx,
        )
    }
}
