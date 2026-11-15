//! Library home and In progress: hero, shelves, progress rows, resume action.
use crate::*;

impl Shell {
    pub(crate) fn shelf(
        &self,
        title: &'static str,
        id: &'static str,
        books: &[&audible::models::LibraryItem],
        cover_side: f32,
        target: Page,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(SectionHeader::new(format!("{id}-all"), title).action(
                "See all",
                cx.listener(move |this, _, _, cx| {
                    this.page = target;
                    this.navigation_generation += 1;
                    cx.notify();
                }),
            ))
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
                        let detail = item
                            .listening_status
                            .as_ref()
                            .and_then(|status| status.time_remaining_seconds)
                            .filter(|_| book_progress(item).unwrap_or(0.) > 0.)
                            .map(|seconds| {
                                model::format::time_left(std::time::Duration::from_secs(seconds))
                            })
                            .unwrap_or(author);
                        div()
                            .id(format!("{id}-{asin}"))
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
                                Cover::new(
                                    cover
                                        .map(gpui_kit::ImageSource::from)
                                        .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                    CoverSize::Square(cover_side),
                                    ui::metrics::radius::COVER,
                                )
                                .elevation(Elevation::CoverShelf),
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
                                    .type_style(ui::text::FOOTNOTE)
                                    .text_color(theme.colors.ink_secondary)
                                    .child(detail),
                            )
                    })),
            )
            .into_any_element()
    }

    /// The resume action for a book: open the player when it is playing, play
    /// when it is on this Mac, otherwise download it.
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
        let muted = cx.theme().colors.ink_secondary;
        if self
            .download
            .as_ref()
            .is_some_and(|download| download.asin == asin)
        {
            return div()
                .type_style(ui::text::BODY)
                .text_color(muted)
                .child("Downloading…")
                .into_any_element();
        }
        if self
            .player
            .as_ref()
            .is_some_and(|player| player.asin == asin)
        {
            return Button::new(format!("{id}-player"))
                .label("Now playing")
                .icon(Icon::Play)
                .variant(variant)
                .on_click(cx.listener(|this, _, _, cx| this.open_player(cx)))
                .into_any_element();
        }
        if backend::downloaded_path(&key, &asin).is_some() {
            return Button::new(format!("{id}-resume"))
                .label("Resume")
                .icon(Icon::Play)
                .variant(variant)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.start_player(key.clone(), asin.clone(), title.clone(), cx)
                }))
                .into_any_element();
        }
        if self.download.is_some() {
            return IconButton::new(format!("{id}-download"), Icon::Download, 16.)
                .hit_area(30.)
                .tooltip("Another download is in progress")
                .into_any_element();
        }
        Button::new(format!("{id}-download"))
            .label("Download")
            .icon(Icon::Download)
            .variant(variant)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.download(key.clone(), asin.clone(), title.clone(), cx)
            }))
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
        let narrator = item.narrator_names();
        let credit = if narrator.is_empty() {
            item.author_names()
        } else {
            format!("{}, read by {narrator}", item.author_names())
        };
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
                                div()
                                    .mt(px(3.))
                                    .truncate()
                                    .type_style(ui::text::BODY)
                                    .text_color(theme.colors.ink_secondary)
                                    .child(credit),
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
            let narrator = item.narrator_names();
            let credit = if narrator.is_empty() {
                item.author_names()
            } else {
                format!("{}, read by {narrator}", item.author_names())
            };
            let cover = self.covers.get(&item.asin).cloned();
            let sentence = item
                .listening_status
                .as_ref()
                .and_then(|status| status.time_remaining_seconds)
                .map(|seconds| {
                    format!(
                        "{}.",
                        model::format::book_left_long(std::time::Duration::from_secs(seconds))
                    )
                })
                .or_else(|| {
                    book_progress(item).map(|percent| format!("{percent:.0}% of the way through."))
                });
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
                                    .text_color(color)
                                    .child("Continue listening"),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .type_style(ui::text::BOOK_DISPLAY)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .type_style(ui::text::CALLOUT)
                                    .text_color(color)
                                    .child(credit),
                            )
                            .children(sentence.map(|sentence| {
                                div()
                                    .mt(px(18.))
                                    .type_style(ui::text::BOOK_SENTENCE)
                                    .child(sentence)
                            }))
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
            view = view.child(
                self.shelf(
                    "Still listening",
                    "listening",
                    &listening
                        .iter()
                        .skip(1)
                        .take(6)
                        .copied()
                        .collect::<Vec<_>>(),
                    160.,
                    Page::Progress,
                    cx,
                ),
            );
        }
        view = view.child(self.shelf(
            "Added recently",
            "recent",
            &recent,
            118.,
            Page::AllBooks,
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
