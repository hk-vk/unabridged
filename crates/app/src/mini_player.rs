//! A small floating window that shares the active player with the main app.
use super::*;

/// Mini player window size, as in the design.
pub(super) const SIZE: (f32, f32) = (420., 132.);

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

/// Keeps the mini player above every app, on every Space, with no title bar.
///
/// GPUI opens floating windows as panels that AppKit hides whenever the app
/// goes to the background; the native helper turns that off.
pub(super) fn float_everywhere(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        unsafe extern "C" {
            fn ba_window_float_everywhere(view: *mut std::ffi::c_void);
        }
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
        {
            // SAFETY: `ns_view` is the live content view of this window, and
            // GPUI calls us on the main thread.
            unsafe { ba_window_float_everywhere(appkit.ns_view.as_ptr()) }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
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
                .type_style(ui::text::BODY)
                .child("Nothing playing")
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
        let chapter = current_chapter(&active.chapters, (position * 1000.) as u64);
        let subtitle = chapter
            .map(|(index, chapter)| {
                if chapter.title.is_empty() {
                    format!("Chapter {}", index + 1)
                } else {
                    chapter.title.clone()
                }
            })
            .unwrap_or_else(|| "Now playing".into());
        let left = match chapter {
            Some((_, chapter)) => {
                let end = (chapter.start_offset_ms.saturating_add(chapter.length_ms) as f64
                    / 1000.)
                    .min(duration);
                format!(
                    "{} left",
                    model::format::clock(std::time::Duration::from_secs(
                        (end - position).max(0.) as u64
                    ))
                )
            }
            None => model::format::time_left(std::time::Duration::from_secs(
                (duration - position).max(0.) as u64,
            )),
        };
        let speed = active.player.rate();
        let seek_shell = self.shell.downgrade();
        let scrubber = Scrubber::new(
            "mini-seek",
            fraction,
            ScrubberColors::field(&palette, false),
        )
        .thickness(4., 10.)
        .on_seek(move |value, _, cx| {
            let _ = seek_shell.update(cx, |shell, cx| shell.seek_fraction(value, cx));
        });
        let back_shell = self.shell.clone();
        let play_shell = self.shell.clone();
        let forward_shell = self.shell.clone();
        let speed_shell = self.shell.clone();
        let open_shell = self.shell.clone();
        // Cover and titles double as the drag handle: the window has no title bar.
        let drag = |el: gpui_kit::Stateful<gpui_kit::Div>| {
            el.on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
                window.start_window_move()
            })
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(16.))
            .p(px(14.))
            .font_family(ui::typography::family::UI)
            .bg(palette.field.opacity(0.86))
            .text_color(palette.on_field)
            .child(
                drag(div().id("mini-cover").flex_none()).child(
                    Cover::new(
                        cover
                            .map(gpui_kit::ImageSource::from)
                            .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                        CoverSize::Square(104.),
                        ui::metrics::radius::COVER_LARGE,
                    )
                    .elevation(Elevation::CoverOnField),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(104.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(
                                drag(div().id("mini-titles"))
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .truncate()
                                            .type_style(ui::text::BOOK_MINI)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .type_style(ui::text::CAPTION)
                                            .text_color(palette.on_field_muted)
                                            .child(subtitle),
                                    ),
                            )
                            .child(
                                IconButton::new("mini-expand", Icon::Expand, 14.)
                                    .hit_area(24.)
                                    .color(palette.on_field_muted)
                                    .hover_fill(palette.on_field_track)
                                    .tooltip("Open full player")
                                    .on_click(move |_, _, cx| {
                                        open_shell
                                            .update(cx, |shell, cx| shell.show_main_player(cx))
                                    }),
                            ),
                    )
                    .child(scrubber)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .w(px(64.))
                                    .type_style(ui::text::CAPTION.tabular())
                                    .text_color(palette.on_field_muted)
                                    .child(left),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(18.))
                                    .child(
                                        IconButton::new("mini-back", Icon::Back30, 23.)
                                            .color(palette.on_field)
                                            .tooltip("Back 30 seconds")
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
                                                diameter: 38.,
                                                icon: 15.,
                                            })
                                            .on_click(move |_, _, cx| {
                                                play_shell.update(cx, |shell, cx| {
                                                    shell.toggle_playback(cx)
                                                })
                                            }),
                                    )
                                    .child(
                                        IconButton::new("mini-forward", Icon::Forward30, 23.)
                                            .color(palette.on_field)
                                            .tooltip("Forward 30 seconds")
                                            .on_click(move |_, _, cx| {
                                                forward_shell
                                                    .update(cx, |shell, cx| shell.skip(30., cx))
                                            }),
                                    ),
                            )
                            .child(
                                div().w(px(64.)).flex().justify_end().child(
                                    div()
                                        .id("mini-speed")
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.72))
                                        .type_style(ui::text::CAPTION_MEDIUM)
                                        .child(model::format::speed(speed))
                                        .on_click(move |_, _, cx| {
                                            speed_shell
                                                .update(cx, |shell, cx| shell.cycle_speed(cx))
                                        }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
