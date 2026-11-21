//! A small floating window that shares the active player with the main app.
use super::*;

/// Mini player window size, as in the design.
pub(crate) const SIZE: (f32, f32) = (420., 132. + TITLEBAR_HEIGHT);

const TITLEBAR_HEIGHT: f32 = if cfg!(target_os = "windows") { 32. } else { 0. };

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
    #[cfg(target_os = "windows")]
    {
        use ::windows::Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{
                HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
            },
        };
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            let _ = unsafe {
                SetWindowPos(
                    HWND(win32.hwnd.get() as *mut _),
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            };
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = window;
}

/// The mini player's height when it is open on read along.
pub(crate) const EXPANDED_HEIGHT: f32 = 470. + TITLEBAR_HEIGHT;

/// Resizes the mini player window, top edge fixed, on the sheet curve.
pub(crate) fn set_height(window: &mut Window, height: f32, duration: std::time::Duration) {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        unsafe extern "C" {
            fn ba_window_set_height(view: *mut std::ffi::c_void, height: f64, duration: f64);
        }
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
        {
            // SAFETY: `ns_view` is the live content view of this window, and
            // GPUI calls us on the main thread.
            unsafe {
                ba_window_set_height(
                    appkit.ns_view.as_ptr(),
                    height as f64,
                    duration.as_secs_f64(),
                )
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = duration;
        window.resize(size(window.bounds().size.width, px(height)));
    }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

// The two shapes, in points. The pill: a 104 cover beside titles and the
// transport. Read along: the cover shrinks into a 44 chip beside the titles,
// the transcript fills the middle and the transport spans the bottom.
const INSET: f32 = 14.;
const COVER_PILL: f32 = 104.;
const COVER_CHIP: f32 = 44.;
/// Scrubber, gap and transport row, measured from the pill's column.
const TRANSPORT_HEIGHT: f32 = 57.;

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_player(window, cx);
        #[cfg(target_os = "windows")]
        return crate::chrome::frame(content, window, cx, true);
        #[cfg(not(target_os = "windows"))]
        content
    }
}

impl MiniPlayer {
    fn render_player(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
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
        // 0 is the pill, 1 is read along; the layout eases between the two
        // in step with the native window resize.
        let (open, moving) = shell.mini_expansion();
        if moving {
            window.request_animation_frame();
        }
        let lyrics_on = shell.mini_lyrics;
        let title = active.title.clone();
        let cover = shell.covers.get(&active.asin).cloned();
        let palette = cx
            .theme()
            .book_palette(shell.palettes.get(&active.asin).copied());
        let playing = active.player.is_playing();
        // The chapter by default; a click on the time switches to the whole book.
        let chapter_timeline = active.timeline();
        let has_chapter = chapter_timeline.chapter.is_some();
        let subtitle = chapter_timeline
            .chapter_name()
            .unwrap_or("Now playing")
            .to_string();
        let book_time = shell.mini_book_time || !has_chapter;
        let timeline = if book_time {
            Timeline::book(active.player.position(), active.player.duration())
        } else {
            chapter_timeline
        };
        let fraction = timeline.fraction();
        let left = format!("{} left", model::format::clock(timeline.remaining()));
        let left_tip = match (has_chapter, book_time) {
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
        // The transcript fades in once the window has mostly opened, rising a
        // little as it comes; folding back, it is gone before the pill returns.
        let lyrics_shown = ((open - 0.3) / 0.7).clamp(0., 1.);
        let lyrics = (lyrics_shown > 0.).then(|| {
            player::read_along_list(
                shell,
                player::ReadAlongPlace::MiniPlayer,
                &palette,
                self.shell.downgrade(),
                window,
                cx,
            )
        });
        let seek_shell = self.shell.downgrade();
        let scrubber = Scrubber::new(
            "mini-seek",
            fraction,
            ScrubberColors::field(&palette, false),
        )
        .thickness(4., 10.)
        .on_seek(move |value, _, cx| {
            let target = timeline.book_fraction(value);
            let _ = seek_shell.update(cx, |shell, cx| shell.seek_fraction(target, cx));
        });
        // Each control talks to the main window's shell.
        let on_shell = |action: fn(&mut Shell, &mut Context<Shell>)| {
            let shell = self.shell.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut gpui_kit::App| {
                shell.update(cx, action)
            }
        };
        let toggle_shell = self.shell.clone();
        // Cover and titles double as the drag handle: the window has no title bar.
        let drag = |el: gpui_kit::Stateful<gpui_kit::Div>| {
            el.on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
                window.start_window_move()
            })
        };

        // Read along keeps only what you reach for while reading: the
        // scrubber, skip back, play and skip forward. Everything else fades
        // out in the first half of the opening, keeping its space so the
        // transport doesn't shift.
        let extras = (1. - open * 2.).clamp(0., 1.);
        let extras_on = extras > 0.;
        let cover_size = lerp(COVER_PILL, COVER_CHIP, open);
        let header_left = INSET + cover_size + lerp(16., 12., open);
        // The titles centre on the chip once it has shrunk.
        let header_top = lerp(INSET, INSET + 4., open);
        let transport_left = lerp(INSET + COVER_PILL + 16., 18., open);
        let transport_right = lerp(INSET, 18., open);
        // The transcript runs right up to the header and transport; its lines
        // fade out at both edges instead of being cut off.
        let lyrics_top = INSET + COVER_CHIP + 2.;
        let lyrics_bottom = INSET + TRANSPORT_HEIGHT + 4.;
        // The pill's field; it deepens towards the bottom as the window opens.
        let background = linear_gradient(
            180.,
            linear_color_stop(palette.field.opacity(0.86), 0.),
            linear_color_stop(
                playback::mix(palette.field, palette.field_deep, open)
                    .opacity(lerp(0.86, 0.92, open)),
                1.,
            ),
        );

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .font_family(ui::typography::family::UI)
            .bg(background)
            .text_color(palette.on_field)
            .when_some(lyrics, |root, lyrics| {
                root.child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .top(px(lyrics_top + (1. - lyrics_shown) * 10.))
                        .bottom(px(lyrics_bottom))
                        .opacity(lyrics_shown)
                        .child(lyrics),
                )
            })
            .child(
                drag(div().id("mini-cover"))
                    .absolute()
                    .left(px(INSET))
                    .top(px(INSET))
                    .child(
                        Cover::new(
                            cover
                                .map(gpui_kit::ImageSource::from)
                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                            CoverSize::Square(cover_size),
                            ui::metrics::radius::COVER_LARGE,
                        )
                        .elevation(Elevation::CoverOnField),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(header_left))
                    .right(px(INSET))
                    .top(px(header_top))
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
                                IconButton::new("mini-read-along", Icon::Subtitle, 13.)
                                    .hit_area(22.)
                                    .color(if lyrics_on {
                                        palette.on_field
                                    } else {
                                        palette.on_field_muted
                                    })
                                    .hover_fill(palette.on_field_track)
                                    .tooltip(if lyrics_on {
                                        "Hide read along"
                                    } else {
                                        "Read along"
                                    })
                                    .on_click(move |_, window, cx| {
                                        let (height, duration) = toggle_shell
                                            .update(cx, |shell, cx| shell.toggle_mini_lyrics(cx));
                                        set_height(window, height, duration);
                                        window.refresh();
                                    }),
                            )
                            .when(extras_on, |icons| {
                                icons.child(
                                    div().opacity(extras).child(
                                        IconButton::new("mini-expand", Icon::Expand, 13.)
                                            .hit_area(22.)
                                            .color(palette.on_field_muted)
                                            .hover_fill(palette.on_field_track)
                                            .tooltip("Open full player")
                                            .on_click(on_shell(|shell, cx| {
                                                shell.show_main_player(cx)
                                            })),
                                    ),
                                )
                            })
                            .child(
                                IconButton::new("mini-close", Icon::Cancel, 13.)
                                    .hit_area(22.)
                                    .color(palette.on_field_muted)
                                    .hover_fill(palette.on_field_track)
                                    .tooltip("Close mini player")
                                    .on_click(on_shell(|shell, cx| shell.close_mini_player(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(transport_left))
                    .right(px(transport_right))
                    .bottom(px(INSET))
                    .flex()
                    .flex_col()
                    .gap(px(11.))
                    .child(scrubber)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(div().w(px(70.)).flex().opacity(extras).when(
                                extras_on,
                                |slot| {
                                    slot.child(
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
                                    )
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .child(div().size(px(22.)).opacity(extras).when(
                                        extras_on,
                                        |slot| {
                                            slot.child(
                                                IconButton::new(
                                                    "mini-previous",
                                                    Icon::Previous,
                                                    15.,
                                                )
                                                .hit_area(22.)
                                                .color(palette.on_field_muted)
                                                .tooltip("Previous chapter")
                                                .on_click(on_shell(|shell, cx| {
                                                    shell.step_chapter(false, cx)
                                                })),
                                            )
                                        },
                                    ))
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
                                    .child(div().size(px(22.)).opacity(extras).when(
                                        extras_on,
                                        |slot| {
                                            slot.child(
                                                IconButton::new("mini-next", Icon::Next, 15.)
                                                    .hit_area(22.)
                                                    .color(palette.on_field_muted)
                                                    .tooltip("Next chapter")
                                                    .on_click(on_shell(|shell, cx| {
                                                        shell.step_chapter(true, cx)
                                                    })),
                                            )
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .w(px(70.))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(4.))
                                    .opacity(extras)
                                    .when(extras_on, |slot| {
                                        slot.child(
                                            IconButton::new("mini-sleep", Icon::Moon, 14.)
                                                .hit_area(22.)
                                                .color(if sleeping {
                                                    palette.on_field
                                                } else {
                                                    palette.on_field_muted
                                                })
                                                .hover_fill(palette.on_field_track)
                                                .tooltip(sleep_tip)
                                                .on_click(on_shell(|shell, cx| {
                                                    shell.cycle_sleep(cx)
                                                })),
                                        )
                                        .child(
                                            div()
                                                .id("mini-speed")
                                                .cursor_pointer()
                                                .hover(|style| style.opacity(0.72))
                                                .type_style(ui::text::CAPTION_MEDIUM)
                                                .child(model::format::speed(speed))
                                                .on_click(on_shell(|shell, cx| {
                                                    shell.cycle_speed(cx)
                                                })),
                                        )
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_heights_include_the_titlebar_once() {
        let titlebar = if cfg!(target_os = "windows") { 32. } else { 0. };
        assert_eq!(SIZE, (420., 132. + titlebar));
        assert_eq!(EXPANDED_HEIGHT, 470. + titlebar);
        assert_eq!(EXPANDED_HEIGHT - SIZE.1, 338.);
    }
}
