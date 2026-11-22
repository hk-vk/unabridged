//! The Now Playing screen.
use crate::*;

impl Shell {
    pub(crate) fn player_view(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(active) = &self.player else {
            return div().into_any_element();
        };
        let compact = f32::from(window.viewport_size().width) < 1000.;
        let cover_size = if compact { 190. } else { 320. };
        let content_gap = if compact { 24. } else { 64. };
        let gutter = if compact { 24. } else { 56. };
        // Content, progress and the control bar share one centred column, so
        // their edges line up.
        let measure = cover_size + content_gap + 560.;
        let book = match &self.account {
            Account::Ready(library) => library.items.iter().find(|item| item.asin == active.asin),
            _ => None,
        };
        let subtitle = book.and_then(|item| item.subtitle.clone());
        let image = self.covers.get(&active.asin).cloned();
        let palette = cx
            .theme()
            .book_palette(self.palettes.get(&active.asin).copied());
        let timeline = active.timeline();
        let fraction = timeline.fraction();
        // The bar is the chapter: time into it on the left, time left in it
        // on the right, and underneath, when it ends on the clock (at the
        // current speed) and how much of the book is left.
        let time = model::format::clock(timeline.elapsed());
        let chapter_countdown = model::format::countdown(timeline.remaining());
        let ends = ends_at(timeline.remaining(), active.player.rate());
        let book_left = model::format::book_left_long(timeline.book_remaining());
        let progress_note = if active.player.is_buffering() {
            "Loading…".to_string()
        } else if timeline.chapter.is_some() {
            format!("Chapter ends at {ends} · {}", lowercase_first(&book_left))
        } else {
            format!("Ends at {ends}")
        };
        let title = active.title.clone();
        let cover = |side: f32| {
            Cover::new(
                image
                    .clone()
                    .map(gpui_kit::ImageSource::from)
                    .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                CoverSize::Square(side),
                ui::metrics::radius::COVER_LARGE,
            )
            .elevation(Elevation::CoverStage)
        };
        let weak = cx.entity().downgrade();
        let scrubber = Scrubber::new(
            "full-player-seek",
            fraction,
            ScrubberColors::field(&palette, true),
        )
        .hit_area(24.)
        .on_seek({
            let timeline = timeline.clone();
            move |value, _, cx| {
                let target = timeline.book_fraction(value);
                let _ = weak.update(cx, |this, cx| this.seek_fraction(target, cx));
            }
        });
        let field_pill = ChipStyle::Outline {
            outline: palette.on_field_track,
            content: palette.on_field,
        };
        let field_pill_muted = ChipStyle::Outline {
            outline: palette.on_field_track,
            content: palette.on_field_muted,
        };
        let volume_weak = cx.entity().downgrade();
        let volume_control = Scrubber::new(
            "full-volume",
            active.player.volume(),
            ScrubberColors::field(&palette, false),
        )
        .on_seek(move |volume, _, cx| {
            let _ = volume_weak.update(cx, |this, cx| this.set_volume(volume, cx));
        });
        let downloading = self.download.as_ref().map(|download| {
            let event = download
                .event
                .lock()
                .map(|event| *event)
                .unwrap_or(audible::DownloadEvent::Licensing);
            let (stage, detail, _) = download_display(event);
            format!("{stage}: {} · {detail}", download.title)
        });
        let stage = div()
            .size_full()
            // Now Playing renders outside the window frame, so it handles
            // the text size and search shortcuts itself.
            .on_action(
                cx.listener(|this, _: &ReadAlongLarger, _, cx| this.step_read_along_size(1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ReadAlongSmaller, _, cx| this.step_read_along_size(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                if !this.read_along_open {
                    this.read_along_open = true;
                    this.ensure_read_along(cx);
                }
                window.focus(
                    &gpui_kit::Focusable::focus_handle(&this.transcript_search, cx),
                    cx,
                );
                cx.notify();
            }))
            .flex()
            .flex_col()
            .font_family(ui::typography::family::UI)
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.field, 0.),
                linear_color_stop(palette.field_deep, 1.),
            ))
            .text_color(palette.on_field)
            .child(
                div()
                    .relative()
                    .h(px(52.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(84.))
                    .pr(px(20.))
                    .gap(px(10.))
                    .children(timeline.chapter.as_ref().map(|(index, name)| {
                        let total = active.chapters.len();
                        // The name truncates; "21 of 30" never does.
                        let trigger = div()
                            .id("current-chapter")
                            .min_w_0()
                            .h(px(ui::metrics::size::POPUP))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(ui::metrics::radius::POPUP))
                            .bg(palette.on_field_track)
                            .cursor_pointer()
                            .hover(|style| style.opacity(0.86))
                            .type_style(ui::text::BODY_MEDIUM)
                            .text_color(palette.on_field)
                            .child(
                                ui::icon(Icon::Chapters)
                                    .flex_none()
                                    .size(px(ui::metrics::icon::MEDIUM))
                                    .text_color(palette.on_field_muted),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(chapter_name(name).to_string()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(palette.on_field_muted)
                                    .child(format!("{} of {total}", index + 1)),
                            );
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .max_w(px(if compact { 260. } else { 420. }))
                                    .flex()
                                    .child(self.chapter_picker(
                                        "current-chapter-menu",
                                        gpui_kit::Anchor::TopCenter,
                                        menus::PickerColors::field(&palette),
                                        trigger,
                                        cx,
                                    )),
                            )
                    }))
                    .child(
                        BackLink::new("player-back", self.back_label())
                            .color(palette.on_field_muted)
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    )
                    .child(div().flex_1().min_w_0())
                    .child(
                        IconButton::new("open-mini", Icon::Pip, 17.)
                            .hit_area(28.)
                            .color(palette.on_field_muted)
                            .hover_fill(palette.on_field_track)
                            .tooltip("Open mini player")
                            .on_click(cx.listener(|this, _, _, cx| this.open_mini_player(cx))),
                    ),
            )
            .children(self.download_notice.as_ref().map(|message| {
                div()
                    .mx(px(if compact { 24. } else { 56. }))
                    .mt(px(8.))
                    .px(px(14.))
                    .py(px(9.))
                    .rounded(px(8.))
                    .bg(palette.on_field_track)
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .type_style(ui::text::CAPTION)
                            .text_color(palette.on_field)
                            .child(message.clone()),
                    )
                    .child(
                        IconButton::new("dismiss-player-status", Icon::Cancel, 14.)
                            .hit_area(24.)
                            .color(palette.on_field_muted)
                            .hover_fill(palette.on_field_track)
                            .tooltip("Dismiss")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.download_notice = None;
                                cx.notify();
                            })),
                    )
            }))
            .children(downloading.map(|message| {
                div()
                    .mx(px(if compact { 24. } else { 56. }))
                    .mt(px(8.))
                    .type_style(ui::text::CAPTION)
                    .text_color(palette.on_field_muted)
                    .child(message)
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(gutter))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(if self.read_along_open {
                        // Cover, title and credits on the left; the transcript
                        // fills the right, top-aligned with the cover.
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .max_w(px(measure))
                            .pt(px(28.))
                            .flex()
                            .gap(px(content_gap))
                            .child(
                                div()
                                    .w(px(cover_size))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .child(cover(cover_size))
                                    .child(
                                        div()
                                            .mt(px(24.))
                                            .type_style(ui::text::BOOK_TITLE)
                                            .child(title.clone()),
                                    )
                                    .children(book.map(|item| {
                                        super::credits(
                                            item,
                                            ui::text::CALLOUT,
                                            palette.on_field_muted,
                                            palette.on_field,
                                        )
                                        .mt(px(6.))
                                    })),
                            )
                            .child({
                                let query =
                                    self.transcript_search.read(cx).value().trim().to_string();
                                let search_summary = self.read_along.as_ref().and_then(|read| {
                                    let job = read.job.lock().ok()?;
                                    (!job.lines.is_empty()).then(|| {
                                        if query.is_empty() {
                                            None
                                        } else {
                                            Some(transcript_matches(&job.lines, &query).len())
                                        }
                                    })
                                });
                                // The bar shows while searching; otherwise a
                                // small search button floats over the
                                // transcript's top-right corner.
                                let search_open = !query.is_empty()
                                    || gpui_kit::Focusable::focus_handle(
                                        &self.transcript_search,
                                        cx,
                                    )
                                    .is_focused(window);
                                let search_button = (search_summary.is_some() && !search_open)
                                    .then(|| {
                                        div().absolute().top_0().right_0().child(
                                            IconButton::new(
                                                "open-transcript-search",
                                                Icon::Search,
                                                15.,
                                            )
                                            .hit_area(30.)
                                            .color(palette.on_field_muted)
                                            .hover_fill(palette.on_field_track)
                                            .tooltip("Search this chapter (⌘F)")
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    window.focus(
                                                        &gpui_kit::Focusable::focus_handle(
                                                            &this.transcript_search,
                                                            cx,
                                                        ),
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }),
                                            ),
                                        )
                                    });
                                let search_bar =
                                    search_summary.filter(|_| search_open).map(|match_count| {
                                        self.transcript_search_bar(
                                            &palette,
                                            match_count,
                                            window,
                                            cx,
                                        )
                                    });
                                div()
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .flex()
                                    .flex_col()
                                    .children(search_bar)
                                    .child(
                                        // The hover fill hangs 8pt outside the text, so
                                        // the text lines up with the column edge.
                                        div().flex_1().min_w_0().min_h_0().ml(px(-8.)).child(
                                            read_along_list(
                                                self,
                                                ReadAlongPlace::NowPlaying,
                                                &palette,
                                                cx.entity().downgrade(),
                                                window,
                                                cx,
                                            ),
                                        ),
                                    )
                                    // Drawn last so it sits over the lines.
                                    .children(search_button)
                            })
                    } else {
                        // Cover and the book's name side by side, centred.
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .max_w(px(measure))
                            .flex()
                            .items_center()
                            .gap(px(content_gap))
                            .child(cover(cover_size))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .max_w(px(560.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .type_style(if compact {
                                                ui::text::BOOK_TITLE
                                            } else {
                                                ui::text::BOOK_DISPLAY
                                            })
                                            .child(title.clone()),
                                    )
                                    .children(subtitle.map(|subtitle| {
                                        div()
                                            .mt(px(12.))
                                            .type_style(ui::text::LEAD)
                                            .text_color(palette.on_field_muted)
                                            .child(subtitle)
                                    }))
                                    .children(book.map(|item| {
                                        super::credits(
                                            item,
                                            ui::text::LEAD,
                                            palette.on_field_muted,
                                            palette.on_field,
                                        )
                                        .mt(px(10.))
                                    })),
                            )
                    })
                    .child(
                        // Progress sits right above the controls: time in, when
                        // the chapter ends, time left in it.
                        div()
                            .flex_none()
                            .w_full()
                            .max_w(px(measure))
                            .mt(px(25.))
                            .child(scrubber)
                            .child(
                                div()
                                    .flex()
                                    .gap(px(14.))
                                    .type_style(ui::text::CAPTION.tabular())
                                    .text_color(palette.on_field_muted)
                                    .child(div().flex_none().child(time))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .justify_center()
                                            .child(div().min_w_0().truncate().child(progress_note)),
                                    )
                                    .child(div().flex_none().child(chapter_countdown)),
                            ),
                    )
                    .child(self.player_dock(
                        compact,
                        measure,
                        &palette,
                        field_pill,
                        field_pill_muted,
                        volume_control,
                        cx,
                    )),
            );
        // The player rises into place; reduced motion shows it immediately.
        if cx.reduce_motion() {
            return stage.into_any_element();
        }
        gpui_kit::base::animation::EffectTransition::new(std::time::Duration::from_millis(220))
            .slide_y(px(12.), px(0.))
            .fade(0.88, 1.0)
            .apply(
                stage.relative(),
                format!("player-enter-{}", self.navigation_generation),
            )
            .into_any_element()
    }
}

impl Shell {
    /// The control bar: speed, sleep and read along on the left, the transport
    /// in the middle, bookmark and volume on the right.
    #[allow(clippy::too_many_arguments)]
    fn player_dock(
        &self,
        compact: bool,
        measure: f32,
        palette: &ui::theme::BookPalette,
        field_pill: ChipStyle,
        field_pill_muted: ChipStyle,
        volume_control: Scrubber,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let Some(active) = &self.player else {
            return div();
        };
        let playing = active.player.is_playing();
        let speed = active.player.rate();
        let sleep_off = matches!(self.sleep, SleepTimer::Off);
        // In a narrow window Sleep and Read along drop their labels so the
        // left cluster never runs into the transport.
        let mut sleep_chip = Chip::new("full-sleep")
            .icon(
                Icon::Moon,
                if sleep_off {
                    palette.on_field_muted
                } else {
                    palette.on_field
                },
            )
            .height(ui::metrics::size::FIELD_PILL)
            .style(if sleep_off {
                field_pill_muted
            } else {
                field_pill
            });
        if !compact {
            sleep_chip = sleep_chip.label(self.sleep.chip_label(std::time::Instant::now()));
        }
        // Read along keeps the pill shape in both states: muted when off,
        // filled when on.
        let (read_style, read_content) = if self.read_along_open {
            (
                ChipStyle::OutlineSelected {
                    fill: palette.on_field_track,
                    content: palette.on_field,
                },
                palette.on_field,
            )
        } else {
            (field_pill_muted, palette.on_field_muted)
        };
        let mut read_chip = Chip::new("show-read-along")
            .icon(Icon::Subtitle, read_content)
            .height(ui::metrics::size::FIELD_PILL)
            .style(read_style)
            .on_click(cx.listener(|this, _, _, cx| {
                this.read_along_open = !this.read_along_open;
                if this.read_along_open {
                    this.ensure_read_along(cx);
                }
                cx.notify();
            }));
        if !compact {
            read_chip = read_chip.label("Read along");
        }
        div()
            .h(px(112.))
            .flex_none()
            .w_full()
            .max_w(px(measure))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        self.speed_picker(
                            "full-speed-menu",
                            gpui_kit::Anchor::BottomLeft,
                            menus::PickerColors::field(palette),
                            Chip::new("full-speed")
                                .label(model::format::speed(speed))
                                .height(ui::metrics::size::FIELD_PILL)
                                .style(field_pill)
                                .text(ui::text::BODY_MEDIUM),
                            cx,
                        ),
                    )
                    .child(self.sleep_picker(
                        "full-sleep-menu",
                        gpui_kit::Anchor::BottomLeft,
                        menus::PickerColors::field(palette),
                        sleep_chip,
                        cx,
                    ))
                    .child(read_chip)
                    .when(self.read_along_open, |el| {
                        el.child(self.read_along_size_control(palette, cx))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(if compact { 24. } else { 40. }))
                    .child(
                        IconButton::new("full-back30", Icon::Back30, 32.)
                            .hit_area(40.)
                            .color(palette.on_field)
                            .tooltip("Back 30 seconds")
                            .on_click(cx.listener(|this, _, _, cx| this.skip(-30., cx))),
                    )
                    .child(
                        Button::new("full-play")
                            .icon(if playing { Icon::Pause } else { Icon::Play })
                            .variant(ButtonVariant::LightLarge(*palette))
                            .shape(ButtonShape::Circle {
                                diameter: 72.,
                                icon: 26.,
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_playback(cx))),
                    )
                    .child(
                        IconButton::new("full-forward30", Icon::Forward30, 32.)
                            .hit_area(40.)
                            .color(palette.on_field)
                            .tooltip("Forward 30 seconds")
                            .on_click(cx.listener(|this, _, _, cx| this.skip(30., cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(18.))
                    .children(
                        active
                            .is_streaming()
                            .then(|| {
                                self.download_icon_in(
                                    "full",
                                    &active.account_key,
                                    &active.asin,
                                    &active.title,
                                    (
                                        palette.on_field_muted,
                                        palette.on_field_muted,
                                        palette.on_field_track,
                                    ),
                                    cx,
                                )
                            })
                            .flatten(),
                    )
                    .child(
                        IconButton::new("add-bookmark", Icon::BookmarkAdd, 18.)
                            .hit_area(28.)
                            .color(palette.on_field_muted)
                            .hover_fill(palette.on_field_track)
                            .tooltip("Add bookmark")
                            .on_click(cx.listener(|this, _, _, cx| this.add_current_bookmark(cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                ui::icon(Icon::VolumeLow)
                                    .flex_none()
                                    .size(px(18.))
                                    .text_color(palette.on_field_muted),
                            )
                            .child(
                                div()
                                    .w(px(if compact { 72. } else { 96. }))
                                    .child(volume_control),
                            ),
                    ),
            )
    }
}

impl Shell {
    /// Search bar above the Now Playing transcript for jumping to matching lines.
    fn transcript_search_bar(
        &self,
        palette: &ui::theme::BookPalette,
        match_count: Option<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let palette = *palette;
        self.transcript_search.update(cx, |state, _| {
            state.set_editor_style(gpui_kit::base::input::InputEditorStyle {
                foreground: palette.on_field,
                muted_foreground: palette.on_field_muted,
                selection: palette.on_field.opacity(0.28),
                caret: palette.on_field,
                ..Default::default()
            });
        });
        let focused =
            gpui_kit::Focusable::focus_handle(&self.transcript_search, cx).is_focused(window);
        let searching = match_count.is_some();
        let count_label = match_count.map(|count| match count {
            0 => "No matches".to_string(),
            1 => "1 match".to_string(),
            n => format!("{n} matches"),
        });
        div()
            .id("transcript-search-bar")
            .flex_none()
            .mb(px(14.))
            .h(px(36.))
            .px(px(12.))
            .rounded(px(10.))
            .bg(palette.on_field_track)
            .border_1()
            .border_color(if focused {
                palette.on_field_muted.opacity(0.6)
            } else {
                palette.on_field_track
            })
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(
                    &gpui_kit::Focusable::focus_handle(&this.transcript_search, cx),
                    cx,
                );
            }))
            .child(
                ui::icon(Icon::Search)
                    .flex_none()
                    .size(px(14.))
                    .text_color(if focused {
                        palette.on_field
                    } else {
                        palette.on_field_muted
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .type_style(ui::text::BODY)
                    .text_color(palette.on_field)
                    .child(self.transcript_search.clone()),
            )
            .children(count_label.map(|label| {
                div()
                    .flex_none()
                    .type_style(ui::text::CAPTION.tabular())
                    .text_color(palette.on_field_muted)
                    .child(label)
            }))
            .when(searching, |bar| {
                bar.child(
                    IconButton::new("clear-transcript-search", Icon::Cancel, 13.)
                        .hit_area(24.)
                        .color(palette.on_field_muted)
                        .hover_fill(palette.on_field_track)
                        .tooltip("Clear search (Esc)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_transcript_search(window, cx);
                        })),
                )
            })
            .when(!searching, |bar| {
                bar.child(
                    div()
                        .flex_none()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(5.))
                        .bg(palette.on_field_track)
                        .type_style(ui::text::FOOTNOTE)
                        .text_color(palette.on_field_muted)
                        .child("⌘F"),
                )
            })
    }

    /// Clears the transcript search and closes it back to the dock icon;
    /// read along follows playback again.
    pub(crate) fn close_transcript_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transcript_search
            .update(cx, |state, cx| state.clean(window, cx));
        self.transcript_match_cursor = None;
        self.read_along_follow.followed = None;
        window.blur(cx);
        cx.notify();
    }

    /// Makes the read along text `delta` steps larger and saves it.
    pub(crate) fn step_read_along_size(&mut self, delta: i8, cx: &mut Context<Self>) {
        let current = cx.theme().settings;
        let next = current.with_read_along_step(delta);
        if next != current {
            self.apply_appearance(next, cx);
        }
    }

    /// Smaller and larger "A" buttons for the read along text, in one pill.
    fn read_along_size_control(
        &self,
        palette: &ui::theme::BookPalette,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let settings = cx.theme().settings;
        let at_min = settings.with_read_along_step(-1) == settings;
        let at_max = settings.with_read_along_step(1) == settings;
        let half = |id: &'static str, size: f32, delta: i8, disabled: bool| {
            div()
                .id(id)
                .w(px(30.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .font_family(ui::typography::family::BOOK)
                .text_size(px(size))
                .text_color(if disabled {
                    palette.on_field_muted.opacity(0.45)
                } else {
                    palette.on_field
                })
                .when(!disabled, |el| {
                    el.cursor_pointer()
                        .hover(|style| style.bg(palette.on_field_track))
                        .active(|style| style.opacity(0.72))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.step_read_along_size(delta, cx)),
                        )
                })
                .child("A")
        };
        div()
            .h(px(ui::metrics::size::FIELD_PILL))
            .flex_none()
            .flex()
            .rounded_full()
            .overflow_hidden()
            .border_1()
            .border_color(palette.on_field_track)
            .child(half("read-along-smaller", 13., -1, at_min))
            .child(div().w(px(1.)).h_full().bg(palette.on_field_track))
            .child(half("read-along-larger", 18., 1, at_max))
    }
}

/// Audible's chapter titles often repeat their own number ("13. Increase
/// Your Surface Area for Luck"), which disagrees with the position count
/// beside it. Drops a leading "13. ", "13: " or "13 - ".
fn chapter_name(name: &str) -> &str {
    let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() == name.len() {
        return name;
    }
    [". ", ": ", " - ", ") "]
        .iter()
        .find_map(|sep| rest.strip_prefix(sep))
        .map(str::trim_start)
        .filter(|rest| !rest.is_empty())
        .unwrap_or(name)
}

/// The wall-clock time that `remaining` audio finishes at `rate`: "9:32 PM".
fn ends_at(remaining: std::time::Duration, rate: f32) -> String {
    use chrono::Timelike;
    let seconds = remaining.as_secs_f64() / f64::from(rate.max(0.1));
    let at = chrono::Local::now() + chrono::Duration::seconds(seconds.round() as i64);
    model::format::time_of_day(model::book::TimeOfDay {
        hour: at.hour() as u8,
        minute: at.minute() as u8,
    })
}

/// "About 2 hours left in the book" → "about 2 hours left in the book".
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Where a transcript is shown; each place keeps its own scroll and style.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadAlongPlace {
    NowPlaying,
    MiniPlayer,
}

/// The read-along transcript: the spoken line bright, the rest soft,
/// following playback. Click a line to jump there.
///
/// In the mini player the spoken line also fills in word by word as it is
/// read, lines already heard fall back, and lines further ahead fade out.
pub(crate) fn read_along_list(
    shell: &Shell,
    place: ReadAlongPlace,
    palette: &ui::theme::BookPalette,
    entity: gpui_kit::WeakEntity<Shell>,
    window: &Window,
    cx: &gpui_kit::App,
) -> gpui_kit::AnyElement {
    let mini = place == ReadAlongPlace::MiniPlayer;
    let follow = if mini {
        &shell.mini_follow
    } else {
        &shell.read_along_follow
    };
    let reduce_motion = cx.reduce_motion();
    let text_scale = cx.theme().settings.read_along_scale();
    let message = |text: &str| {
        div()
            .type_style(ui::text::BODY)
            .text_color(palette.on_field_muted)
            .child(text.to_string())
            .into_any_element()
    };
    let Some(active) = &shell.player else {
        return div().into_any_element();
    };
    // An outlined action under a message: Download, Try again.
    let action = |id: &'static str, label: &'static str, glyph: Icon| {
        Chip::new(id)
            .label(label)
            .icon(glyph, palette.on_field)
            .height(ui::metrics::size::POPUP)
            .style(ChipStyle::Outline {
                outline: palette.on_field_track,
                content: palette.on_field,
            })
    };
    let with_action = |text: &str, chip: Chip| {
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(14.))
            .child(message(text))
            .child(chip)
            .into_any_element()
    };
    if !active.is_streaming()
        && backend::downloaded_path(&active.account_key, &active.asin).is_none()
    {
        // Read along needs the audio on this Mac; offer the download here
        // and start transcribing once it lands.
        return match &shell.download {
            Some(download) if download.asin == active.asin => {
                message("Downloading this book. Read along starts when it’s on this Mac.")
            }
            Some(_) => message(
                "Read along works for books downloaded to this Mac. You can download this one when the current download finishes.",
            ),
            None => {
                let key = active.account_key.clone();
                let asin = active.asin.clone();
                let title = active.title.clone();
                let entity = entity.clone();
                with_action(
                    "Read along works for books downloaded to this Mac.",
                    action("read-along-download", "Download", Icon::Download).on_click(
                        move |_, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.download(key.clone(), asin.clone(), title.clone(), cx)
                            });
                        },
                    ),
                )
            }
        };
    }
    let Some(read) = &shell.read_along else {
        return transcript_placeholder("Getting ready", palette, mini, reduce_motion);
    };
    let Ok(job) = read.job.lock() else {
        return div().into_any_element();
    };
    match &job.phase {
        transcript::Phase::Checking if job.lines.is_empty() => {
            return transcript_placeholder("Getting ready", palette, mini, reduce_motion);
        }
        transcript::Phase::NeedsModel => {
            let entity = entity.clone();
            let language = transcript::speech_name(read.locale);
            return div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(
                    div()
                        .type_style(ui::text::BODY)
                        .text_color(palette.on_field_muted)
                        .child(format!(
                            "Read along uses your Mac’s built-in speech recognition. macOS needs to add {language} speech first. It’s a one-time download that macOS keeps. Nothing leaves your Mac."
                        )),
                )
                .child(
                    div().flex().child(
                        Chip::new("download-speech-model")
                            .label(format!("Add {language} speech"))
                            .icon(Icon::Download, palette.on_field)
                            .height(ui::metrics::size::POPUP)
                            .style(ChipStyle::Outline {
                                outline: palette.on_field_track,
                                content: palette.on_field,
                            })
                            .on_click(move |_, _, cx| {
                                let _ = entity.update(cx, |this, cx| this.download_speech_model(cx));
                            }),
                    ),
                )
                .into_any_element();
        }
        transcript::Phase::Downloading(progress) => {
            return div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .w(px(if mini { 240. } else { 280. }))
                .child(
                    div()
                        .type_style(ui::text::CAPTION.tabular())
                        .text_color(palette.on_field_muted)
                        .child(format!(
                            "Adding {} speech… {:.0}%",
                            transcript::speech_name(read.locale),
                            progress * 100.
                        )),
                )
                .child(
                    SegmentedProgress::new(0., *progress, 0.)
                        .colors(palette.on_field, palette.on_field_track),
                )
                .into_any_element();
        }
        transcript::Phase::Fetching(progress) if job.lines.is_empty() => {
            return div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .w(px(if mini { 240. } else { 280. }))
                .child(
                    div()
                        .type_style(ui::text::CAPTION.tabular())
                        .text_color(palette.on_field_muted)
                        .child(format!(
                            "Getting this chapter ready… {:.0}%",
                            progress * 100.
                        )),
                )
                .child(
                    SegmentedProgress::new(0., *progress, 0.)
                        .colors(palette.on_field, palette.on_field_track),
                )
                .into_any_element();
        }
        transcript::Phase::Unsupported(text) if job.lines.is_empty() => {
            return message(text);
        }
        transcript::Phase::Failed(text) if job.lines.is_empty() => {
            let entity = entity.clone();
            return with_action(
                text,
                action("read-along-retry", "Try again", Icon::Refresh).on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| this.retry_read_along(cx));
                }),
            );
        }
        transcript::Phase::Transcribing if job.lines.is_empty() => {
            return transcript_placeholder(
                "Transcribing this chapter",
                palette,
                mini,
                reduce_motion,
            );
        }
        _ => {}
    }
    let position = active.player.position();
    let duration = active.player.duration().max(1.);
    let current = transcript::current_line(&job.lines, position);
    let still_working = job.phase == transcript::Phase::Transcribing;
    let query = if mini {
        String::new()
    } else {
        shell.transcript_search.read(cx).value().trim().to_string()
    };
    if !query.is_empty() {
        let chapter_start = active.timeline().start;
        let matches = transcript_matches(&job.lines, &query);
        if matches.is_empty() {
            return div()
                .px(px(8.))
                .pt(px(8.))
                .type_style(ui::text::BODY)
                .text_color(palette.on_field_muted)
                .child(if still_working {
                    ShimmerText::new("No matches yet — still transcribing…").into_any_element()
                } else {
                    "No matches in this chapter.".into_any_element()
                })
                .into_any_element();
        }
        let mut results = div()
            .id("read-along-search")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&shell.transcript_search_scroll)
            .flex()
            .flex_col()
            .gap(px(8.))
            .pb(px(120.));
        for (match_pos, line_index) in matches.into_iter().enumerate() {
            let line = &job.lines[line_index];
            let target = (line.start / duration).clamp(0., 1.) as f32;
            let timestamp = model::format::clock(std::time::Duration::from_secs_f64(
                (line.start - chapter_start).max(0.),
            ));
            let is_current =
                current == Some(line_index) || shell.transcript_match_cursor == Some(match_pos);
            let base_color = if is_current {
                palette.on_field
            } else {
                palette.on_field_muted
            };
            results = results.child(
                div()
                    .id(("read-along-match", line_index))
                    .cursor_pointer()
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .when(is_current, |row| row.bg(palette.on_field_track))
                    .hover(|style| style.bg(palette.on_field_track))
                    .flex()
                    .items_baseline()
                    .gap(px(12.))
                    .on_click({
                        let entity = entity.clone();
                        // Jump there and go back to following along.
                        move |_, window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.seek_fraction(target, cx);
                                this.mini_follow.followed = None;
                                this.close_transcript_search(window, cx);
                            });
                        }
                    })
                    .child(
                        div()
                            .flex_none()
                            .w(px(48.))
                            .type_style(ui::text::CAPTION.tabular())
                            .text_color(palette.on_field_muted)
                            .child(timestamp),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .type_style(ui::text::BOOK_CAPTION.scaled(text_scale))
                            .text_color(base_color)
                            .child(highlight_matches(
                                &line.text,
                                &query,
                                base_color,
                                palette.on_field,
                            )),
                    ),
            );
        }
        if still_working {
            results = results.child(
                div()
                    .px(px(8.))
                    .type_style(ui::text::CAPTION)
                    .text_color(palette.on_field_muted)
                    .child(ShimmerText::new("Still transcribing this chapter…")),
            );
        }
        return results.into_any_element();
    }
    // Glide the list and crossfade the highlight between lines.
    let gliding = follow.step_glide();
    let fade = follow.changed_at.elapsed().as_secs_f32() / playback::HIGHLIGHT_FADE.as_secs_f32();
    if gliding || fade < 1. {
        window.request_animation_frame();
    }
    let fade = fade.min(1.);
    let spoken = palette.on_field;
    let upcoming = palette.on_field_muted;
    // Now Playing keeps heard lines at full secondary contrast; the mini
    // player lets them fall back.
    let heard = if mini {
        palette.on_field_muted.opacity(0.55)
    } else {
        upcoming
    };
    let id = if mini {
        "mini-read-along"
    } else {
        "read-along"
    };
    // Each line fades itself near the edges of the list, by where it sat last
    // frame (laid out without the scroll, so add the offset back): top and
    // bottom in the mini player, the bottom only in Now Playing.
    let viewport = follow.scroll.bounds();
    let offset = follow.scroll.offset().y;
    let edge_fade = |index: usize| -> f32 {
        if viewport.size.height <= px(0.) {
            return 1.;
        }
        let Some(item) = follow.scroll.bounds_for_item(index) else {
            return 1.;
        };
        let height = viewport.size.height;
        let top = (item.origin.y + offset - viewport.origin.y) / height;
        let bottom = (item.origin.y + item.size.height + offset - viewport.origin.y) / height;
        // Now Playing only fades toward the progress row below it.
        let from_top = if mini {
            (bottom / 0.16).clamp(0., 1.)
        } else {
            1.
        };
        let from_bottom = ((1. - top) / 0.22).clamp(0., 1.);
        let smooth = |t: f32| t * t * (3. - 2. * t);
        smooth(from_top.min(from_bottom))
    };
    let mut list = div()
        .id(id)
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&follow.scroll)
        .flex()
        .flex_col()
        .gap(px(if mini { 10. } else { 14. }))
        .when(mini, |list| list.pt(px(40.)).pb(px(240.)))
        .when(!mini, |list| list.pb(px(120.)));
    for (index, line) in job.lines.iter().enumerate() {
        let target = (line.start / duration).clamp(0., 1.) as f32;
        let resting = |at_line: Option<usize>| match at_line {
            Some(at) if at == index => spoken,
            Some(at) if index < at => heard,
            // In the mini player, lines further ahead recede.
            Some(at) if mini && index > at + 2 => upcoming.opacity(0.6),
            _ => upcoming,
        };
        let color = if follow.followed == current {
            playback::mix(resting(follow.previous), resting(current), fade)
        } else {
            resting(current)
        };
        let row = div()
            .id((id, index))
            .opacity(edge_fade(index))
            .cursor_pointer()
            .px(px(8.))
            .py(px(if mini { 3. } else { 4. }))
            .rounded(px(8.))
            .hover(|style| style.bg(palette.on_field_track))
            .type_style(
                if mini {
                    ui::text::BOOK_READ_ALONG
                } else {
                    ui::text::BOOK_CAPTION
                }
                .scaled(text_scale),
            )
            .text_color(color)
            .on_click({
                let entity = entity.clone();
                move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        this.seek_fraction(target, cx);
                        this.read_along_follow.followed = None;
                        this.mini_follow.followed = None;
                    });
                }
            });
        // The spoken line fills in word by word, timed to the sentence.
        let filling = mini && !reduce_motion && current == Some(index);
        list = list.child(if filling {
            let span = (line.end - line.start).max(0.1);
            let progress = ((position - line.start) / span).clamp(0., 1.) as f32;
            let unread = playback::mix(upcoming, spoken, 0.35);
            row.child(word_fill(&line.text, progress, unread, color))
        } else {
            row.child(line.text.clone())
        });
    }
    if still_working {
        list = list.child(
            div()
                .px(px(8.))
                .type_style(ui::text::CAPTION)
                .text_color(palette.on_field_muted)
                .child(ShimmerText::new("Transcribing…")),
        );
    }
    list.into_any_element()
}

/// A line whose words light up from `unread` to `read` as `progress` sweeps
/// across it, each word easing in over its own share of the line.
fn word_fill(
    text: &str,
    progress: f32,
    unread: gpui_kit::Hsla,
    read: gpui_kit::Hsla,
) -> gpui_kit::StyledText {
    let total = text.chars().count().max(1) as f32;
    let reached = progress * total;
    let mut highlights = Vec::new();
    let mut chars_before = 0usize;
    let mut word_start: Option<(usize, usize)> = None;
    let mut push = |start: usize, end: usize, first_char: usize, chars: usize| {
        let share = ((reached - first_char as f32) / chars.max(1) as f32).clamp(0., 1.);
        highlights.push((
            start..end,
            gpui_kit::HighlightStyle {
                color: Some(playback::mix(unread, read, share)),
                ..Default::default()
            },
        ));
    };
    for (byte, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some((start, first_char)) = word_start.take() {
                push(start, byte, first_char, chars_before - first_char);
            }
        } else if word_start.is_none() {
            word_start = Some((byte, chars_before));
        }
        chars_before += 1;
    }
    if let Some((start, first_char)) = word_start {
        push(start, text.len(), first_char, chars_before - first_char);
    }
    gpui_kit::StyledText::new(text.to_string()).with_highlights(highlights)
}

/// Shimmering label and line-shaped bars while a transcript is on its way.
fn transcript_placeholder(
    label: &'static str,
    palette: &ui::theme::BookPalette,
    mini: bool,
    reduced_motion: bool,
) -> gpui_kit::AnyElement {
    let scale = if mini { 0.78 } else { 1. };
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .when(mini, |column| column.pt(px(18.)).px(px(8.)))
        .child(
            div()
                .type_style(ui::text::CAPTION)
                .text_color(palette.on_field_muted)
                .child(ShimmerText::new(label)),
        );
    for (index, width) in [420., 360., 400., 300., 380.].into_iter().enumerate() {
        let width = width * scale;
        column = column.child(if reduced_motion {
            div()
                .w(px(width))
                .h(px(16.))
                .rounded(px(8.))
                .bg(palette.on_field_track)
                .into_any_element()
        } else {
            div()
                .id(("transcript-loading", index))
                .child(Skeleton::new().w(px(width)).h(px(16.)).rounded(px(8.)))
                .into_any_element()
        });
    }
    column.into_any_element()
}

/// Indices of transcript lines matching `query` (case-insensitive).
pub(crate) fn transcript_matches(lines: &[transcript::Line], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (!match_ranges(&line.text, query).is_empty()).then_some(index))
        .collect()
}

/// Non-overlapping byte ranges in `text` matching `query` case-insensitively,
/// guaranteed to fall on UTF-8 char boundaries of `text`.
fn match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let haystack: Vec<(usize, usize, char)> = text
        .char_indices()
        .flat_map(|(start, ch)| {
            let end = start + ch.len_utf8();
            ch.to_lowercase().map(move |lower| (start, end, lower))
        })
        .collect();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if (0..needle.len()).all(|j| haystack[i + j].2 == needle[j]) {
            let start = haystack[i].0;
            let end = haystack[i + needle.len() - 1].1;
            ranges.push(start..end);
            i += needle.len();
            while i < haystack.len() && haystack[i].0 < end {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    ranges
}

/// Draws `text` with `query` matches emphasized in `emphasis`.
fn highlight_matches(
    text: &str,
    query: &str,
    base: gpui_kit::Hsla,
    emphasis: gpui_kit::Hsla,
) -> gpui_kit::StyledText {
    let ranges = match_ranges(text, query);
    let mut highlights = Vec::new();
    let mut cursor = 0;
    for range in ranges {
        if cursor < range.start {
            highlights.push((
                cursor..range.start,
                gpui_kit::HighlightStyle {
                    color: Some(base),
                    ..Default::default()
                },
            ));
        }
        highlights.push((
            range.clone(),
            gpui_kit::HighlightStyle {
                color: Some(emphasis),
                font_weight: Some(gpui_kit::FontWeight::SEMIBOLD),
                underline: Some(gpui_kit::UnderlineStyle {
                    thickness: px(1.),
                    color: Some(emphasis.opacity(0.5)),
                    wavy: false,
                }),
                ..Default::default()
            },
        ));
        cursor = range.end;
    }
    if cursor < text.len() {
        highlights.push((
            cursor..text.len(),
            gpui_kit::HighlightStyle {
                color: Some(base),
                ..Default::default()
            },
        ));
    }
    gpui_kit::StyledText::new(text.to_string()).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::{chapter_name, match_ranges, transcript_matches};
    use crate::transcript::Line;

    #[test]
    fn chapter_names_drop_their_own_number() {
        assert_eq!(
            chapter_name("13. Increase Your Surface Area for Luck"),
            "Increase Your Surface Area for Luck"
        );
        assert_eq!(chapter_name("2: Start"), "Start");
        assert_eq!(chapter_name("Chapter 3"), "Chapter 3");
        assert_eq!(chapter_name("1984"), "1984");
        assert_eq!(chapter_name("12 Rules"), "12 Rules");
    }

    #[test]
    fn transcript_search_matches_case_insensitively_and_safely() {
        let text = "Feel-Good Productivity makes productivity feel natural.";
        let ranges = match_ranges(text, "productivity");
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].clone()], "Productivity");
        assert_eq!(&text[ranges[1].clone()], "productivity");

        // Turkish dotted I expands in lowercase; slices must stay on char boundaries.
        let unicode = "İstanbul and malayalam മലയാളം";
        let i_ranges = match_ranges(unicode, "i");
        assert_eq!(i_ranges.len(), 1);
        assert_eq!(&unicode[i_ranges[0].clone()], "İ");
        let ml_ranges = match_ranges(unicode, "മലയാളം");
        assert_eq!(ml_ranges.len(), 1);
        assert_eq!(&unicode[ml_ranges[0].clone()], "മലയാളം");

        let lines = vec![
            Line {
                start: 0.,
                end: 3.,
                text: "First line about habits.".into(),
            },
            Line {
                start: 3.,
                end: 6.,
                text: "Second line about focus.".into(),
            },
            Line {
                start: 6.,
                end: 9.,
                text: "Building a better HABIT loop.".into(),
            },
        ];
        assert_eq!(transcript_matches(&lines, "habit"), vec![0, 2]);
        assert!(transcript_matches(&lines, "   ").is_empty());
        assert!(transcript_matches(&lines, "nonexistent").is_empty());
    }
}
