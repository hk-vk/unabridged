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
        let chapter_width = if compact { 180. } else { 260. };
        let content_gap = if compact { 24. } else { 64. };
        let book = match &self.account {
            Account::Ready(library) => library.items.iter().find(|item| item.asin == active.asin),
            _ => None,
        };
        let author = book.map(|item| item.author_names()).unwrap_or_default();
        let narrator = book.map(|item| item.narrator_names()).unwrap_or_default();
        let credit = if narrator.is_empty() {
            author
        } else {
            format!("{author}, read by {narrator}")
        };
        let image = self.covers.get(&active.asin).cloned();
        let palette = cx
            .theme()
            .book_palette(self.palettes.get(&active.asin).copied());
        let timeline = active.timeline();
        let caption = match &timeline.chapter {
            Some((index, name)) => format!(
                "left in {name}, chapter {} of {}",
                index + 1,
                active.chapters.len()
            ),
            None => "left in this book".to_string(),
        };
        let fraction = timeline.fraction();
        let time = model::format::clock(timeline.elapsed());
        let book_left = model::format::book_left_long(timeline.book_remaining());
        let left = model::format::clock(timeline.remaining());
        let playing = active.player.is_playing();
        let speed = active.player.rate();
        let title = active.title.clone();
        let weak = cx.entity().downgrade();
        let scrubber = Scrubber::new(
            "full-player-seek",
            fraction,
            ScrubberColors::field(&palette, true),
        )
        .on_seek({
            let timeline = timeline.clone();
            move |value, _, cx| {
                let target = timeline.book_fraction(value);
                let _ = weak.update(cx, |this, cx| this.seek_fraction(target, cx));
            }
        });
        let chapter_panel = if self.chapters_open {
            let message = "Chapters unavailable for this book.";
            let mut panel = div()
                .id("chapter-list")
                .w(px(chapter_width))
                .h_full()
                .flex_none()
                .overflow_y_scroll()
                .p(px(16.))
                .rounded(px(10.))
                .bg(palette.on_field_track)
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().type_style(ui::text::GROUP_TITLE).child("Chapters"));
            if active.chapters_loading && active.chapters.is_empty() {
                let reduced_motion = cx.reduce_motion();
                for index in 0_usize..8 {
                    let width = [180., 140., 200., 160.][index % 4];
                    panel = panel.child(
                        div()
                            .id(("chapter-loading", index))
                            .flex()
                            .justify_between()
                            .items_center()
                            .py(px(10.))
                            .px(px(8.))
                            .child(if reduced_motion {
                                div()
                                    .w(px(width))
                                    .h(px(12.))
                                    .rounded(px(6.))
                                    .bg(palette.on_field_track)
                                    .into_any_element()
                            } else {
                                Skeleton::new()
                                    .w(px(width))
                                    .h(px(12.))
                                    .rounded(px(6.))
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .w(px(40.))
                                    .h(px(12.))
                                    .rounded(px(6.))
                                    .bg(palette.on_field_track),
                            ),
                    );
                }
            } else if active.chapters.is_empty() {
                panel = panel.child(
                    div()
                        .mt(px(12.))
                        .type_style(ui::text::BODY)
                        .text_color(palette.on_field_muted)
                        .child(message),
                );
            }
            for (index, chapter) in active.chapters.iter().enumerate() {
                let name = if chapter.title.is_empty() {
                    format!("Chapter {}", index + 1)
                } else {
                    chapter.title.clone()
                };
                let start = chapter.start_offset_ms as f64 / 1000.;
                let target = if timeline.duration > 0. {
                    (start / timeline.duration).clamp(0., 1.) as f32
                } else {
                    0.
                };
                panel = panel.child(
                    div()
                        .id(("chapter", index))
                        .cursor_pointer()
                        .py(px(10.))
                        .px(px(8.))
                        .rounded(px(7.))
                        .type_style(ui::text::BODY)
                        .text_color(palette.on_field)
                        .hover(|style| style.bg(palette.on_field_track))
                        .on_click(cx.listener(move |this, _, _, cx| this.seek_fraction(target, cx)))
                        .child(name),
                );
            }
            let panel = if cx.reduce_motion() {
                panel.into_any_element()
            } else {
                gpui_kit::base::animation::EffectTransition::new(std::time::Duration::from_millis(
                    140,
                ))
                .slide_x(px(10.), px(0.))
                .fade(0.90, 1.0)
                .apply(panel.relative(), "chapter-panel-enter")
                .into_any_element()
            };
            Some(panel)
        } else {
            None
        };
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
            .flex()
            .flex_col()
            .font_family("Geist")
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.field, 0.),
                linear_color_stop(palette.field_deep, 1.),
            ))
            .text_color(palette.on_field)
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(84.))
                    .pr(px(20.))
                    .gap(px(10.))
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
                    )
                    .child(
                        Chip::new("show-read-along")
                            .label("Read along")
                            .icon(Icon::Subtitle, palette.on_field)
                            .height(ui::metrics::size::POPUP)
                            .style(ChipStyle::Tint {
                                fill: if self.read_along_open {
                                    palette.on_field_track
                                } else {
                                    gpui_kit::transparent_black()
                                },
                                content: palette.on_field,
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.read_along_open = !this.read_along_open;
                                if this.read_along_open {
                                    this.chapters_open = false;
                                    this.ensure_read_along(cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Chip::new("show-chapters")
                            .label("Chapters")
                            .icon(Icon::Chapters, palette.on_field)
                            .height(ui::metrics::size::POPUP)
                            .style(if self.chapters_open {
                                ChipStyle::Tint {
                                    fill: palette.on_field_track,
                                    content: palette.on_field,
                                }
                            } else {
                                ChipStyle::Tint {
                                    fill: gpui_kit::transparent_black(),
                                    content: palette.on_field,
                                }
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.chapters_open = !this.chapters_open;
                                if this.chapters_open {
                                    this.read_along_open = false;
                                }
                                cx.notify();
                            })),
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
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(content_gap))
                    .px(px(if compact { 24. } else { 56. }))
                    .child(
                        Cover::new(
                            image
                                .map(gpui_kit::ImageSource::from)
                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                            CoverSize::Square(cover_size),
                            ui::metrics::radius::COVER_LARGE,
                        )
                        .elevation(Elevation::CoverStage),
                    )
                    .child(
                        div()
                            .flex_1()
                            .max_w(px(560.))
                            .flex()
                            .flex_col()
                            .child(div().type_style(ui::text::BOOK_TITLE).child(title))
                            .child(
                                div()
                                    .mt(px(6.))
                                    .type_style(ui::text::CALLOUT)
                                    .text_color(palette.on_field_muted)
                                    .child(credit),
                            )
                            .when(self.read_along_open, |column| {
                                column.child(
                                    div()
                                        .mt(px(28.))
                                        .h(px(if compact { 220. } else { 340. }))
                                        .child(self.read_along_view(&palette, window, cx)),
                                )
                            })
                            .when(!self.read_along_open, |column| {
                                column
                                    .child(
                                        div()
                                            .mt(px(40.))
                                            .font_family("Geist Pixel")
                                            .text_size(px(if compact { 70. } else { 104. }))
                                            .child(left),
                                    )
                                    .child(
                                        div()
                                            .mt(px(8.))
                                            .type_style(ui::text::BOOK_CAPTION)
                                            .child(caption),
                                    )
                            })
                            .child(div().mt(px(32.)).child(scrubber))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .mt(px(6.))
                                    .type_style(ui::text::CAPTION)
                                    .text_color(palette.on_field_muted)
                                    .child(time)
                                    .child(book_left),
                            ),
                    )
                    .children(chapter_panel),
            )
            .child(
                div()
                    .h(px(if compact { 112. } else { 140. }))
                    .flex_none()
                    .px(px(if compact { 24. } else { 56. }))
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
                                Chip::new("full-speed")
                                    .label(model::format::speed(speed))
                                    .height(ui::metrics::size::FIELD_PILL)
                                    .style(field_pill)
                                    .text(ui::text::BODY_MEDIUM)
                                    .on_click(cx.listener(|this, _, _, cx| this.cycle_speed(cx))),
                            )
                            .child(
                                Chip::new("full-sleep")
                                    .label(self.sleep.chip_label(std::time::Instant::now()))
                                    .icon(Icon::Moon, palette.on_field)
                                    .height(ui::metrics::size::FIELD_PILL)
                                    .style(if matches!(self.sleep, SleepTimer::Off) {
                                        field_pill_muted
                                    } else {
                                        field_pill
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.cycle_sleep(cx))),
                            ),
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
                                    .variant(ButtonVariant::LightLarge(palette))
                                    .shape(ButtonShape::Circle {
                                        diameter: 72.,
                                        icon: 26.,
                                    })
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_playback(cx)),
                                    ),
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
                            .child(
                                IconButton::new("add-bookmark", Icon::BookmarkAdd, 18.)
                                    .hit_area(28.)
                                    .color(palette.on_field_muted)
                                    .hover_fill(palette.on_field_track)
                                    .tooltip("Add bookmark")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.add_current_bookmark(cx)),
                                    ),
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
                                    .child(div().w(px(96.)).child(volume_control)),
                            ),
                    ),
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
    /// The read-along transcript: the spoken line bright, the rest soft,
    /// following playback. Click a line to jump there.
    fn read_along_view(
        &self,
        palette: &ui::theme::BookPalette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let message = |text: &'static str| {
            div()
                .type_style(ui::text::BODY)
                .text_color(palette.on_field_muted)
                .child(text)
                .into_any_element()
        };
        let Some(active) = &self.player else {
            return div().into_any_element();
        };
        if backend::downloaded_path(&active.account_key, &active.asin).is_none() {
            return message("Read along works for books downloaded to this Mac.");
        }
        let Some(read) = &self.read_along else {
            return transcript_placeholder("Getting ready", palette, cx);
        };
        let Ok(job) = read.job.lock() else {
            return div().into_any_element();
        };
        match &job.phase {
            transcript::Phase::Checking if job.lines.is_empty() => {
                return transcript_placeholder("Getting ready", palette, cx);
            }
            transcript::Phase::NeedsModel => {
                return div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .type_style(ui::text::BODY)
                            .text_color(palette.on_field_muted)
                            .child(
                                "Read along transcribes each chapter on your Mac with Apple’s speech model. Nothing leaves your Mac. The model is a one-time download.",
                            ),
                    )
                    .child(
                        div().flex().child(
                            Chip::new("download-speech-model")
                                .label("Download speech model")
                                .icon(Icon::Download, palette.on_field)
                                .height(ui::metrics::size::POPUP)
                                .style(ChipStyle::Outline {
                                    outline: palette.on_field_track,
                                    content: palette.on_field,
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.download_speech_model(cx)
                                })),
                        ),
                    )
                    .into_any_element();
            }
            transcript::Phase::Downloading(progress) => {
                return div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .w(px(280.))
                    .child(
                        div()
                            .type_style(ui::text::CAPTION.tabular())
                            .text_color(palette.on_field_muted)
                            .child(format!("Downloading speech model… {:.0}%", progress * 100.)),
                    )
                    .child(
                        SegmentedProgress::new(0., *progress, 0.)
                            .colors(palette.on_field, palette.on_field_track),
                    )
                    .into_any_element();
            }
            transcript::Phase::Unsupported(text) | transcript::Phase::Failed(text)
                if job.lines.is_empty() =>
            {
                return div()
                    .type_style(ui::text::BODY)
                    .text_color(palette.on_field_muted)
                    .child(text.clone())
                    .into_any_element();
            }
            transcript::Phase::Transcribing if job.lines.is_empty() => {
                return transcript_placeholder("Transcribing this chapter", palette, cx);
            }
            _ => {}
        }
        let position = active.player.position();
        let duration = active.player.duration().max(1.);
        let current = transcript::current_line(&job.lines, position);
        let still_working = job.phase == transcript::Phase::Transcribing;
        // Glide the list and crossfade the highlight between lines.
        let gliding = read.step_glide(&self.read_along_scroll);
        let fade = read.changed_at.elapsed().as_secs_f32() / playback::HIGHLIGHT_FADE.as_secs_f32();
        if gliding || fade < 1. {
            window.request_animation_frame();
        }
        let fade = fade.min(1.);
        let spoken = palette.on_field;
        let upcoming = palette.on_field_muted;
        let heard = palette.on_field_muted.opacity(0.55);
        let mut list = div()
            .id("read-along")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.read_along_scroll)
            .flex()
            .flex_col()
            .gap(px(14.))
            .pb(px(120.));
        for (index, line) in job.lines.iter().enumerate() {
            let target = (line.start / duration).clamp(0., 1.) as f32;
            let resting = |line: Option<usize>| match line {
                Some(at) if at == index => spoken,
                Some(at) if index < at => heard,
                _ => upcoming,
            };
            let color = if read.followed == current {
                playback::mix(resting(read.previous), resting(current), fade)
            } else {
                resting(current)
            };
            list = list.child(
                div()
                    .id(("read-along-line", index))
                    .cursor_pointer()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(8.))
                    .hover(|style| style.bg(palette.on_field_track))
                    .type_style(ui::text::BOOK_ROW)
                    .text_color(color)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.seek_fraction(target, cx);
                        if let Some(read) = &mut this.read_along {
                            read.followed = None;
                        }
                    }))
                    .child(line.text.clone()),
            );
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
}

/// Shimmering label and line-shaped bars while a transcript is on its way.
fn transcript_placeholder(
    label: &'static str,
    palette: &ui::theme::BookPalette,
    cx: &Context<Shell>,
) -> gpui_kit::AnyElement {
    let reduced_motion = cx.reduce_motion();
    let mut column = div().flex().flex_col().gap(px(14.)).child(
        div()
            .type_style(ui::text::CAPTION)
            .text_color(palette.on_field_muted)
            .child(ShimmerText::new(label)),
    );
    for (index, width) in [420., 360., 400., 300., 380.].into_iter().enumerate() {
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
