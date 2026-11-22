//! Library home and In progress: hero, shelves, progress rows, resume action.
use crate::*;

impl Shell {
    /// A row of covers under a section heading. `resume_key` adds a hover
    /// play button to books in progress.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shelf(
        &self,
        title: &'static str,
        id: &'static str,
        books: &[&audible::models::LibraryItem],
        cover_side: f32,
        target: Page,
        detail: Option<String>,
        resume_key: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let mut header = SectionHeader::new(format!("{id}-all"), title).action(
            "See all",
            cx.listener(move |this, _, _, cx| {
                this.page = target;
                this.navigation_generation += 1;
                cx.notify();
            }),
        );
        if let Some(detail) = detail {
            header = header.detail(detail);
        }
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(20.))
                    .children(books.iter().map(|item| {
                        let asin = item.asin.clone();
                        let open_asin = asin.clone();
                        let name = item.title.clone().unwrap_or_else(|| "Untitled".into());
                        let author = item.author_names();
                        let cover = self.covers.get(&asin).cloned();
                        let percent = book_progress(item).unwrap_or(0.);
                        let in_progress = percent > 0. && !book_finished(item);
                        let remaining = item
                            .listening_status
                            .as_ref()
                            .and_then(|status| status.time_remaining_seconds)
                            .filter(|_| in_progress);
                        // "38% · 5 hr 10 min left"; the last quarter hour reads
                        // "Almost done" in the accent colour.
                        let detail = match remaining {
                            Some(seconds) if seconds < 15 * 60 => div()
                                .flex()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_color(theme.accent.text)
                                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                                        .child("Almost done"),
                                )
                                .child(format!(
                                    "· {}",
                                    model::format::time_left(std::time::Duration::from_secs(
                                        seconds
                                    ))
                                )),
                            Some(seconds) => div().child(format!(
                                "{percent:.0}% · {}",
                                model::format::time_left(std::time::Duration::from_secs(seconds))
                            )),
                            None => div().child(author),
                        };
                        let group: gpui_kit::SharedString = format!("{id}-card-{asin}").into();
                        let resume = resume_key.filter(|_| in_progress).map(|key| {
                            let key = key.to_string();
                            let asin = asin.clone();
                            let title = name.clone();
                            // Shown on hover; resumes without opening the book.
                            div()
                                .id(format!("{id}-play-{asin}"))
                                .absolute()
                                .right(px(8.))
                                .bottom(px(20.))
                                .size(px(34.))
                                .rounded_full()
                                .bg(gpui_kit::white().opacity(0.94))
                                .shadow_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(0.)
                                .group_hover(group.clone(), |style| style.opacity(1.))
                                .cursor_pointer()
                                .child(
                                    ui::icon(Icon::Play)
                                        .size(px(14.))
                                        .text_color(theme.colors.window),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.resume_book(key.clone(), asin.clone(), title.clone(), cx);
                                }))
                        });
                        div()
                            .id(format!("{id}-{asin}"))
                            .group(group.clone())
                            .w(px(cover_side))
                            .flex_none()
                            .cursor_pointer()
                            .hover(|style| style.opacity(0.86))
                            .active(|style| style.opacity(0.72))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_book(open_asin.clone(), cx)
                            }))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .relative()
                                    .child(
                                        Cover::new(
                                            cover
                                                .map(gpui_kit::ImageSource::from)
                                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                            CoverSize::Square(cover_side),
                                            ui::metrics::radius::COVER,
                                        )
                                        .elevation(Elevation::CoverShelf),
                                    )
                                    // Progress on the cover itself: a white bar on
                                    // a dark strip reads on any cover colour.
                                    .when(in_progress, |el| {
                                        el.child(
                                            div()
                                                .absolute()
                                                .left(px(8.))
                                                .right(px(8.))
                                                .bottom(px(8.))
                                                .h(px(4.))
                                                .rounded_full()
                                                .bg(gpui_kit::black().opacity(0.45))
                                                .child(
                                                    div()
                                                        .h_full()
                                                        .w(gpui_kit::relative(
                                                            (percent / 100.).clamp(0., 1.) as f32,
                                                        ))
                                                        .rounded_full()
                                                        .bg(gpui_kit::white()),
                                                ),
                                        )
                                    })
                                    .children(resume),
                            )
                            .child(
                                div()
                                    .mt(px(10.))
                                    .truncate()
                                    .type_style(ui::text::CAPTION_MEDIUM)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .truncate()
                                    .type_style(ui::text::FOOTNOTE.tabular())
                                    .text_color(theme.colors.ink_secondary)
                                    .child(detail),
                            )
                    })),
            )
            .into_any_element()
    }

    /// Resumes a book from a shelf: toggles it when it is in the player,
    /// otherwise plays it (from this Mac, or streamed).
    fn resume_book(
        &mut self,
        key: String,
        asin: audible::Asin,
        title: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .player
            .as_ref()
            .is_some_and(|player| player.asin == asin)
        {
            self.toggle_playback(cx);
        } else {
            self.start_player(key, asin, title, cx);
        }
    }

    /// The small download control beside a book's play button: a download
    /// icon, the download's progress, or nothing once it's on this Mac.
    pub(crate) fn download_icon(
        &self,
        id: &str,
        key: &str,
        asin: &audible::Asin,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let colors = cx.theme().colors;
        if let Some(download) = &self.download
            && &download.asin == asin
        {
            let label = download
                .event
                .lock()
                .map(|event| {
                    let (stage, _, fraction) = download_display(*event);
                    fraction.map_or_else(
                        || format!("{stage}…"),
                        |fraction| format!("{stage} {:.0}%", fraction * 100.),
                    )
                })
                .unwrap_or_else(|_| "Downloading…".into());
            return Some(
                div()
                    .type_style(ui::text::CAPTION.tabular())
                    .text_color(colors.ink_secondary)
                    .child(label)
                    .into_any_element(),
            );
        }

        if backend::downloaded_path(key, asin).is_some() {
            return None;
        }
        let button = IconButton::new(format!("{id}-download"), Icon::Download, 16.)
            .hit_area(30.)
            .hover_fill(colors.fill);
        Some(if self.download.is_some() {
            button
                .color(colors.ink_secondary)
                .tooltip("Another download is in progress")
                .into_any_element()
        } else {
            let (key, asin, title) = (key.to_string(), asin.clone(), title.to_string());
            button
                .color(colors.ink)
                .tooltip("Download to this Mac")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.download(key.clone(), asin.clone(), title.clone(), cx)
                }))
                .into_any_element()
        })
    }

    /// The resume action for a book: pause or play when it's in the player,
    /// otherwise play it (from this Mac, or streamed), with the small
    /// download icon beside it.
    pub(crate) fn resume_action(
        &self,
        id: &str,
        account_key: &str,
        item: &audible::models::LibraryItem,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let variant = if primary {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Secondary
        };
        let asin = item.asin.clone();
        let key = account_key.to_string();
        let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
        let download = self.download_icon(id, &key, &asin, &title, cx);
        let button = if self
            .player
            .as_ref()
            .is_some_and(|player| player.asin == asin)
        {
            let playing = self
                .player
                .as_ref()
                .is_some_and(|player| player.player.is_playing());
            Button::new(format!("{id}-player"))
                .label(if playing { "Pause" } else { "Play" })
                .icon(if playing { Icon::Pause } else { Icon::Play })
                .variant(variant)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_playback(cx)))
        } else {
            let started = book_progress(item).unwrap_or(0.) > 0. && !book_finished(item);
            Button::new(format!("{id}-resume"))
                .label(if started { "Resume" } else { "Play" })
                .icon(Icon::Play)
                .variant(variant)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.start_player(key.clone(), asin.clone(), title.clone(), cx)
                }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .children(download)
            .child(button)
            .into_any_element()
    }

    /// One "In progress" row: cover, title, credit and progress bar, then the
    /// percentage in pixel type, time left, and the resume action.
    pub(crate) fn progress_row(
        &self,
        index: usize,
        account_key: &str,
        item: &audible::models::LibraryItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let asin = item.asin.clone();
        let open_asin = asin.clone();
        let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
        let percent = book_progress(item).unwrap_or(0.);
        let left = item
            .listening_status
            .as_ref()
            .and_then(|status| status.time_remaining_seconds)
            .map(|seconds| model::format::time_left(std::time::Duration::from_secs(seconds)));
        let cover = self.covers.get(&asin).cloned();
        div()
            .flex()
            .items_center()
            .gap(px(24.))
            .py(px(16.))
            .border_b_1()
            .border_color(theme.colors.hairline)
            .child(
                div()
                    .id(format!("progress-open-{asin}"))
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(20.))
                    .cursor_pointer()
                    .active(|style| style.opacity(0.8))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.open_book(open_asin.clone(), cx)),
                    )
                    .child(
                        Cover::new(
                            cover
                                .map(gpui_kit::ImageSource::from)
                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                            CoverSize::Square(88.),
                            ui::metrics::radius::COVER,
                        )
                        .elevation(Elevation::Cover),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(420.))
                            .flex()
                            .flex_col()
                            .child(div().truncate().type_style(ui::text::BOOK_ROW).child(title))
                            .child(
                                super::credits(
                                    item,
                                    ui::text::BODY,
                                    theme.colors.ink_secondary,
                                    theme.colors.ink,
                                )
                                .mt(px(3.)),
                            )
                            .child(div().mt(px(14.)).child(SegmentedProgress::new(
                                0.,
                                (percent / 100.) as f32,
                                0.,
                            ))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(4.))
                    .child(
                        div()
                            .type_style(ui::text::PIXEL_STAT)
                            .child(model::format::percent(percent.round() as u32)),
                    )
                    .children(left.map(|left| {
                        div()
                            .type_style(ui::text::CAPTION)
                            .text_color(theme.colors.ink_secondary)
                            .child(left)
                    })),
            )
            .child(
                div()
                    .w(px(120.))
                    .flex()
                    .justify_end()
                    .child(self.resume_action(
                        &format!("progress-{asin}"),
                        account_key,
                        item,
                        index == 0,
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn home_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let listening: Vec<_> = library
            .items
            .iter()
            .filter(|item| book_progress(item).unwrap_or(0.) > 0. && !book_finished(item))
            .collect();
        let recent: Vec<_> = library.items.iter().take(8).collect();
        let mut view = div().flex().flex_col().gap(px(40.));
        if let Some(item) = listening.first() {
            let details_asin = item.asin.clone();
            let chapters_asin = item.asin.clone();
            let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
            let cover = self.covers.get(&item.asin).cloned();
            let active = self
                .player
                .as_ref()
                .filter(|player| player.asin == item.asin);
            let playing = active.is_some_and(|player| player.player.is_playing());
            // Progress is shown as a bar with a quiet caption, not a sentence.
            let percent = book_progress(item).unwrap_or(0.);
            let left = item
                .listening_status
                .as_ref()
                .and_then(|status| status.time_remaining_seconds)
                .map(|seconds| model::format::time_left(std::time::Duration::from_secs(seconds)));
            // When this book is in the player the bar shows its chapters,
            // weighted by length: done, the one playing, still to come.
            let timeline = active.map(|player| (player.timeline(), player.chapters.len()));
            let bar = match &timeline {
                Some((timeline, _))
                    if timeline.chapter.is_some() && timeline.end > timeline.start =>
                {
                    let span = timeline.end - timeline.start;
                    SegmentedProgress::new(
                        (timeline.start / span) as f32,
                        ((timeline.position - timeline.start) / span) as f32,
                        ((timeline.duration - timeline.end).max(0.) / span) as f32,
                    )
                }
                _ => SegmentedProgress::new(0., (percent / 100.) as f32, 0.),
            };
            let mut caption = vec![format!("{percent:.0}%")];
            if let Some((timeline, total)) = &timeline
                && let Some((index, _)) = &timeline.chapter
            {
                caption.push(format!("chapter {} of {total}", index + 1));
            }
            caption.extend(left);
            let caption = caption.join(" · ");
            view = view.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(36.))
                    .child(
                        div()
                            .id("hero-book")
                            .cursor_pointer()
                            .active(|style| style.opacity(0.86))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_book(details_asin.clone(), cx)
                            }))
                            .child(
                                Cover::new(
                                    cover
                                        .map(gpui_kit::ImageSource::from)
                                        .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                    CoverSize::Square(220.),
                                    ui::metrics::radius::COVER_LARGE,
                                )
                                .elevation(Elevation::CoverHero),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .type_style(ui::text::CAPTION)
                                    .text_color(if playing {
                                        cx.theme().accent.text
                                    } else {
                                        color
                                    })
                                    .child(if playing {
                                        "Playing now"
                                    } else {
                                        "Continue listening"
                                    }),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .type_style(ui::text::BOOK_DISPLAY)
                                    .child(title),
                            )
                            .child(
                                super::credits(
                                    item,
                                    ui::text::CALLOUT,
                                    color,
                                    cx.theme().colors.ink,
                                )
                                .mt(px(8.)),
                            )
                            .child(
                                div()
                                    .mt(px(18.))
                                    .w(px(440.))
                                    .max_w_full()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(bar)
                                    .child(
                                        div()
                                            .type_style(ui::text::CAPTION.tabular())
                                            .text_color(color)
                                            .child(caption),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(20.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(self.resume_action(
                                        "hero",
                                        &library.account_key,
                                        item,
                                        true,
                                        cx,
                                    ))
                                    .child(Button::new("hero-details").label("Details").on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.open_book(chapters_asin.clone(), cx)
                                        }),
                                    )),
                            ),
                    ),
            );
            let others = &listening[1..];
            let summary = (!others.is_empty()).then(|| {
                let seconds: u64 = others
                    .iter()
                    .filter_map(|item| {
                        item.listening_status
                            .as_ref()
                            .and_then(|status| status.time_remaining_seconds)
                    })
                    .sum();
                format!(
                    "{} {} · {}",
                    others.len(),
                    if others.len() == 1 { "book" } else { "books" },
                    model::format::time_left(std::time::Duration::from_secs(seconds))
                )
            });
            view = view.child(self.shelf(
                "Still listening",
                "listening",
                &others.iter().take(6).copied().collect::<Vec<_>>(),
                160.,
                Page::Progress,
                summary,
                Some(&library.account_key),
                cx,
            ));
        }
        view = view.child(self.shelf(
            "Added recently",
            "recent",
            &recent,
            118.,
            Page::AllBooks,
            None,
            None,
            cx,
        ));
        view.into_any_element()
    }

    pub(crate) fn progress_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let hairline = cx.theme().colors.hairline;
        let items: Vec<_> = library
            .items
            .iter()
            .filter(|item| book_progress(item).unwrap_or(0.) > 0. && !book_finished(item))
            .collect();
        if !items.is_empty() {
            let remaining: u64 = items
                .iter()
                .filter_map(|item| {
                    item.listening_status
                        .as_ref()
                        .and_then(|status| status.time_remaining_seconds)
                })
                .sum();
            let key = library.account_key.clone();
            return div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .pb(px(12.))
                        .border_b_1()
                        .border_color(hairline)
                        .type_style(ui::text::BODY)
                        .text_color(color)
                        .child(model::format::library_summary(
                            items.len(),
                            std::time::Duration::from_secs(remaining),
                        )),
                )
                .children(
                    items
                        .iter()
                        .enumerate()
                        .map(|(index, item)| self.progress_row(index, &key, item, cx)),
                )
                .into_any_element();
        }
        self.book_list_view(library, cx)
    }
}
