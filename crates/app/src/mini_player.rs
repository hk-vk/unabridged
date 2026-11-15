//! A small floating window that shares the active player with the main app.
use super::*;

pub(super) struct MiniPlayer {
    shell: Entity<Shell>,
    _subscription: gpui_kit::Subscription,
}

impl MiniPlayer {
    pub(super) fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            _subscription: subscription,
        }
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.read(cx);
        let Some(active) = &shell.player else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child("No book playing")
                .into_any_element();
        };
        let title = active.title.clone();
        let cover = shell.covers.get(&active.asin).cloned();
        let palette = cx
            .theme()
            .book_palette(shell.palettes.get(&active.asin).copied());
        let playing = active.player.is_playing();
        let position = active.player.position().max(0.);
        let duration = active.player.duration().max(0.);
        let fraction = if duration > 0. {
            (position / duration).clamp(0., 1.) as f32
        } else {
            0.
        };
        let remaining = model::format::time_left(std::time::Duration::from_secs(
            (duration - position).max(0.) as u64,
        ));
        let speed = active.player.rate();
        let seek_shell = self.shell.downgrade();
        let scrubber = Scrubber::new("mini-seek", fraction, ScrubberColors::field(&palette, true))
            .on_seek(move |value, _, cx| {
                let _ = seek_shell.update(cx, |shell, cx| shell.seek_fraction(value, cx));
            });
        let back_shell = self.shell.clone();
        let play_shell = self.shell.clone();
        let forward_shell = self.shell.clone();
        let speed_shell = self.shell.clone();
        let open_shell = self.shell.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Geist")
            .bg(palette.field)
            .text_color(palette.on_field)
            .pt(px(30.))
            .px(px(14.))
            .pb(px(12.))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .items_center()
                    .child(
                        Cover::new(
                            cover
                                .map(gpui_kit::ImageSource::from)
                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                            CoverSize::Square(96.),
                            7.,
                        )
                        .elevation(Elevation::CoverOnField),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_family("Literata")
                                            .text_size(px(15.))
                                            .child(title),
                                    )
                                    .child(
                                        Button::new("mini-expand")
                                            .icon(Icon::Expand)
                                            .variant(ButtonVariant::Glass(palette))
                                            .on_click(move |_, _, cx| {
                                                open_shell.update(cx, |shell, cx| {
                                                    shell.show_main_player(cx)
                                                })
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(palette.on_field_muted)
                                    .child(remaining),
                            )
                            .child(scrubber)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(5.))
                                    .child(
                                        Button::new("mini-speed")
                                            .label(format!("{speed}×"))
                                            .variant(ButtonVariant::Glass(palette))
                                            .on_click(move |_, _, cx| {
                                                speed_shell
                                                    .update(cx, |shell, cx| shell.cycle_speed(cx))
                                            }),
                                    )
                                    .child(
                                        Button::new("mini-back")
                                            .icon(Icon::Back30)
                                            .variant(ButtonVariant::Glass(palette))
                                            .shape(ButtonShape::Circle {
                                                diameter: 30.,
                                                icon: 16.,
                                            })
                                            .on_click(move |_, _, cx| {
                                                back_shell
                                                    .update(cx, |shell, cx| shell.skip(-30., cx))
                                            }),
                                    )
                                    .child(
                                        Button::new("mini-play")
                                            .icon(if playing { Icon::Pause } else { Icon::Play })
                                            .variant(ButtonVariant::Light(palette))
                                            .shape(ButtonShape::Circle {
                                                diameter: 32.,
                                                icon: 16.,
                                            })
                                            .on_click(move |_, _, cx| {
                                                play_shell.update(cx, |shell, cx| {
                                                    shell.toggle_playback(cx)
                                                })
                                            }),
                                    )
                                    .child(
                                        Button::new("mini-forward")
                                            .icon(Icon::Forward30)
                                            .variant(ButtonVariant::Glass(palette))
                                            .shape(ButtonShape::Circle {
                                                diameter: 30.,
                                                icon: 16.,
                                            })
                                            .on_click(move |_, _, cx| {
                                                forward_shell
                                                    .update(cx, |shell, cx| shell.skip(30., cx))
                                            }),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
