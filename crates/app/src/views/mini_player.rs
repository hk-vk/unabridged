//! A small floating window that shares the active player with the main app.
use super::*;

/// Mini player window size, as in the design.
pub(crate) const SIZE: (f32, f32) = (420., 132.);

pub(crate) struct MiniPlayer {
    shell: Entity<Shell>,
    _subscription: gpui_kit::Subscription,
}

impl MiniPlayer {
    pub(crate) fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
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
pub(crate) fn float_everywhere(window: &Window) {
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
        // Time left in the chapter by default; a click switches to the book.
        let book_time = shell.mini_book_time || chapter.is_none();
        let end = match chapter {
            Some((_, chapter)) if !book_time => {
                (chapter.start_offset_ms.saturating_add(chapter.length_ms) as f64 / 1000.)
                    .min(duration)
            }
            _ => duration,
        };
        let left = format!(
            "{} left",
            model::format::clock(std::time::Duration::from_secs(
                (end - position).max(0.) as u64
            ))
        );
        let left_tip = match (chapter.is_some(), book_time) {
            (false, _) => "Time left in the book",
            (true, false) => "Time left in this chapter. Click for the book",
            (true, true) => "Time left in the book. Click for this chapter",
        };
        let sleeping = !matches!(shell.sleep, SleepTimer::Off);
        let sleep_tip = if sleeping {
            shell.sleep.label(std::time::Instant::now())
        } else {
            "Sleep timer".into()
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
        // Each control talks to the main window's shell.
        let on_shell = |action: fn(&mut Shell, &mut Context<Shell>)| {
            let shell = self.shell.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut gpui_kit::App| {
                shell.update(cx, action)
            }
        };
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
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .child(
                                        IconButton::new("mini-expand", Icon::Expand, 13.)
                                            .hit_area(22.)
                                            .color(palette.on_field_muted)
                                            .hover_fill(palette.on_field_track)
                                            .tooltip("Open full player")
                                            .on_click(on_shell(|shell, cx| {
                                                shell.show_main_player(cx)
                                            })),
                                    )
                                    .child(
                                        IconButton::new("mini-close", Icon::Cancel, 13.)
                                            .hit_area(22.)
                                            .color(palette.on_field_muted)
                                            .hover_fill(palette.on_field_track)
                                            .tooltip("Close mini player")
                                            .on_click(on_shell(|shell, cx| {
                                                shell.close_mini_player(cx)
                                            })),
                                    ),
                            ),
                    )
                    .child(scrubber)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div().w(px(70.)).flex().child(
                                    div()
                                        .id("mini-time")
                                        .cursor_pointer()
                                        .hover(|style| style.opacity(0.72))
                                        .type_style(ui::text::CAPTION.tabular())
                                        .text_color(palette.on_field_muted)
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(left_tip)
                                                .build(window, cx)
                                        })
                                        .on_click(on_shell(|shell, cx| {
                                            shell.mini_book_time = !shell.mini_book_time;
                                            cx.notify();
                                        }))
                                        .child(left),
                                ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .child(
                                        IconButton::new("mini-previous", Icon::Previous, 15.)
                                            .hit_area(22.)
                                            .color(palette.on_field_muted)
                                            .tooltip("Previous chapter")
                                            .on_click(on_shell(|shell, cx| {
                                                shell.step_chapter(false, cx)
                                            })),
                                    )
                                    .child(
                                        IconButton::new("mini-back", Icon::Back30, 21.)
                                            .hit_area(26.)
                                            .color(palette.on_field)
                                            .tooltip("Back 30 seconds")
                                            .on_click(on_shell(|shell, cx| shell.skip(-30., cx))),
                                    )
                                    .child(
                                        Button::new("mini-play")
                                            .icon(if playing { Icon::Pause } else { Icon::Play })
                                            .variant(ButtonVariant::Light(palette))
                                            .shape(ButtonShape::Circle {
                                                diameter: 36.,
                                                icon: 15.,
                                            })
                                            .on_click(on_shell(|shell, cx| {
                                                shell.toggle_playback(cx)
                                            })),
                                    )
                                    .child(
                                        IconButton::new("mini-forward", Icon::Forward30, 21.)
                                            .hit_area(26.)
                                            .color(palette.on_field)
                                            .tooltip("Forward 30 seconds")
                                            .on_click(on_shell(|shell, cx| shell.skip(30., cx))),
                                    )
                                    .child(
                                        IconButton::new("mini-next", Icon::Next, 15.)
                                            .hit_area(22.)
                                            .color(palette.on_field_muted)
                                            .tooltip("Next chapter")
                                            .on_click(on_shell(|shell, cx| {
                                                shell.step_chapter(true, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(70.))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(4.))
                                    .child(
                                        IconButton::new("mini-sleep", Icon::Moon, 14.)
                                            .hit_area(22.)
                                            .color(if sleeping {
                                                palette.on_field
                                            } else {
                                                palette.on_field_muted
                                            })
                                            .hover_fill(palette.on_field_track)
                                            .tooltip(sleep_tip)
                                            .on_click(on_shell(|shell, cx| shell.cycle_sleep(cx))),
                                    )
                                    .child(
                                        div()
                                            .id("mini-speed")
                                            .cursor_pointer()
                                            .hover(|style| style.opacity(0.72))
                                            .type_style(ui::text::CAPTION_MEDIUM)
                                            .child(model::format::speed(speed))
                                            .on_click(on_shell(|shell, cx| shell.cycle_speed(cx))),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
