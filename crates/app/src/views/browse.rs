//! Library lists: bookmarks, authors and series, and the filtered book lists
//! (On this Mac, Finished, search results, an author or series).
use crate::*;

impl Shell {
    pub(crate) fn bookmarks_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let theme_ink = cx.theme().colors.ink;
        let theme_fill = cx.theme().colors.fill;
        let hairline = cx.theme().colors.hairline;
        let copy = |text: String| copy(text, color);
        let base = page_base;
        if self.bookmarks_loading {
            return loading_placeholders("Loading bookmarks…", cx);
        }
        if self.bookmarks.is_empty() {
            return base().child(heading("No bookmarks yet")).child(copy("Save a moment from the player and find it here. Bookmarks are stored on this Mac.".into())).into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .type_style(ui::text::BODY)
                    .text_color(color)
                    .child("Saved on this Mac"),
            )
            .children(self.bookmarks.iter().filter_map(|(asin, position)| {
                let item = library.items.iter().find(|item| &item.asin == asin)?;
                let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                let time = model::format::clock(std::time::Duration::from_millis(*position));
                let key = library.account_key.clone();
                let play_asin = asin.clone();
                let remove_asin = asin.clone();
                let bookmark_position = *position;
                let downloaded = backend::downloaded_path(&key, asin).is_some();
                let play_title = title.clone();
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .py(px(14.))
                        .border_b_1()
                        .border_color(hairline)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(div().type_style(ui::text::BOOK_ROW).child(title))
                                .child(
                                    div()
                                        .type_style(ui::text::CAPTION)
                                        .text_color(color)
                                        .child(time),
                                ),
                        )
                        .child(if downloaded {
                            IconButton::new(
                                format!("bookmark-play-{asin}-{position}"),
                                Icon::Play,
                                16.,
                            )
                            .hit_area(30.)
                            .color(theme_ink)
                            .hover_fill(theme_fill)
                            .tooltip("Play from here")
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.start_player_at(
                                        key.clone(),
                                        play_asin.clone(),
                                        play_title.clone(),
                                        Some(bookmark_position),
                                        cx,
                                    )
                                },
                            ))
                        } else {
                            IconButton::new(
                                format!("bookmark-book-{asin}-{position}"),
                                Icon::ChevronRight,
                                16.,
                            )
                            .hit_area(30.)
                            .hover_fill(theme_fill)
                            .tooltip("Open book")
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.open_book(play_asin.clone(), cx),
                            ))
                        })
                        .child(
                            IconButton::new(
                                format!("bookmark-remove-{asin}-{position}"),
                                Icon::Delete,
                                16.,
                            )
                            .hit_area(30.)
                            .hover_fill(theme_fill)
                            .tooltip("Remove bookmark")
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.change_bookmark(
                                        remove_asin.clone(),
                                        bookmark_position,
                                        false,
                                        cx,
                                    )
                                },
                            )),
                        ),
                )
            }))
            .into_any_element()
    }

    pub(crate) fn groups_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let hover_fill = cx.theme().colors.fill;
        let copy = |text: String| copy(text, color);
        let base = page_base;
        let authors = self.page == Page::Authors;
        let mut groups = std::collections::BTreeMap::<String, usize>::new();
        for item in &library.items {
            if authors {
                for author in &item.authors {
                    if !author.name.is_empty() {
                        *groups.entry(author.name.clone()).or_default() += 1;
                    }
                }
            } else {
                for series in &item.series {
                    if let Some(title) = &series.title
                        && !title.is_empty()
                    {
                        *groups.entry(title.clone()).or_default() += 1;
                    }
                }
            }
        }
        if groups.is_empty() {
            return base()
                .child(heading(if authors {
                    "No authors found"
                } else {
                    "No series found"
                }))
                .child(copy(
                    "These details are not available in your library yet.".into(),
                ))
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(
                groups
                    .into_iter()
                    .enumerate()
                    .map(|(index, (name, count))| {
                        let selected = name.clone();
                        div()
                            .id(("browse-group", index))
                            .cursor_pointer()
                            .px(px(12.))
                            .py(px(14.))
                            .rounded(px(8.))
                            .hover(|style| style.bg(hover_fill))
                            .active(|style| style.opacity(0.8))
                            .flex()
                            .items_center()
                            .gap(px(20.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.browse_name = Some(selected.clone());
                                this.page = if authors {
                                    Page::AuthorBooks
                                } else {
                                    Page::SeriesBooks
                                };
                                this.navigation_generation += 1;
                                cx.notify();
                            }))
                            .child(div().flex_1().type_style(ui::text::BOOK_ROW).child(name))
                            .child(div().type_style(ui::text::CAPTION).text_color(color).child(
                                format!("{count} {}", if count == 1 { "book" } else { "books" }),
                            ))
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn book_list_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let theme_ink = cx.theme().colors.ink;
        let theme_fill = cx.theme().colors.fill;
        let hairline = cx.theme().colors.hairline;
        let hover_fill = cx.theme().colors.fill;
        let copy = |text: String| copy(text, color);
        let base = page_base;
        let query = self.library_search.read(cx).value().trim().to_lowercase();
        if self.page == Page::Search && query.is_empty() {
            return base()
                .child(copy("Search the books in your library.".into()))
                .into_any_element();
        }
        let items: Vec<_> = library
            .items
            .iter()
            .filter(|item| match self.page {
                Page::Progress => book_progress(item).unwrap_or(0.) > 0. && !book_finished(item),
                Page::OnThisMac => {
                    backend::downloaded_path(&library.account_key, &item.asin).is_some()
                        || self
                            .download
                            .as_ref()
                            .is_some_and(|download| download.asin == item.asin)
                }
                Page::Finished => book_finished(item),
                Page::Search => {
                    item.title
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                        || item.author_names().to_lowercase().contains(&query)
                        || item.narrator_names().to_lowercase().contains(&query)
                }
                Page::AuthorBooks => self
                    .browse_name
                    .as_ref()
                    .is_some_and(|name| item.authors.iter().any(|author| &author.name == name)),
                Page::SeriesBooks => self.browse_name.as_ref().is_some_and(|name| {
                    item.series
                        .iter()
                        .any(|series| series.title.as_ref() == Some(name))
                }),
                _ => true,
            })
            .collect();
        if items.is_empty() {
            return base()
                .child(heading(match self.page {
                    Page::Progress => "Nothing in progress",
                    Page::OnThisMac => "No downloads yet",
                    Page::Search => "No matching books",
                    Page::Finished => "No finished books",
                    _ => "Your library is empty",
                }))
                .child(copy(if self.page == Page::OnThisMac {
                    "Download a title from your library to listen on this Mac.".into()
                } else if self.page == Page::Search {
                    "Try a different title, author or narrator.".into()
                } else if library.offline {
                    "Showing the library saved on this Mac.".into()
                } else {
                    "Your Audible library has synced.".into()
                }))
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(
                matches!(self.page, Page::AuthorBooks | Page::SeriesBooks).then(|| {
                    let target = if self.page == Page::AuthorBooks {
                        Page::Authors
                    } else {
                        Page::Series
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(BackLink::new("browse-back", page_name(target)).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.page = target;
                                cx.notify();
                            }),
                        ))
                        .child(
                            div()
                                .type_style(ui::text::BOOK_TITLE)
                                .child(self.browse_name.clone().unwrap_or_default()),
                        )
                }),
            )
            .child(
                div()
                    .type_style(ui::text::BODY)
                    .text_color(color)
                    .child(format!("{} titles", items.len())),
            )
            .children(items.into_iter().map(|item| {
                let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                let author = item.author_names();
                let image = self.covers.get(&item.asin).cloned();
                let asin = item.asin.clone();
                let key = library.account_key.clone();
                let button_id = format!("download-{}", asin);
                let local_path = backend::downloaded_path(&key, &asin);
                let downloaded = local_path.is_some();
                let active = self
                    .download
                    .as_ref()
                    .filter(|download| download.asin == asin);
                let active_stage = active.map(|download| {
                    download
                        .event
                        .lock()
                        .map(|event| download_display(*event).0)
                        .unwrap_or("Preparing download")
                });
                let other_download = self.download.is_some() && active.is_none();
                let download_title = title.clone();
                let open_asin = asin.clone();
                let open_id = format!("book-{asin}");
                let progress = book_progress(item)
                    .map(|v| format!("{v:.0}% complete"))
                    .unwrap_or_else(|| "Not started".into());
                div()
                    .flex()
                    .items_center()
                    .gap(px(20.))
                    .pb(px(16.))
                    .border_b_1()
                    .border_color(hairline)
                    .child(
                        div()
                            .id(open_id)
                            .cursor_pointer()
                            .rounded(px(8.))
                            .hover(|style| style.bg(hover_fill))
                            .active(|style| style.opacity(0.8))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(20.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_book(open_asin.clone(), cx)
                            }))
                            .child(
                                Cover::new(
                                    image
                                        .map(gpui_kit::ImageSource::from)
                                        .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                    CoverSize::Square(76.),
                                    ui::metrics::radius::COVER,
                                )
                                .elevation(Elevation::CoverThumb),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(5.))
                                    .child(div().type_style(ui::text::BOOK_ROW).child(title))
                                    .child(
                                        div()
                                            .type_style(ui::text::BODY)
                                            .text_color(color)
                                            .child(author),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .type_style(ui::text::BODY)
                            .text_color(color)
                            .child(progress),
                    )
                    .child(if let Some(stage) = active_stage {
                        div()
                            .type_style(ui::text::BODY)
                            .text_color(color)
                            .child(format!("{stage}…"))
                            .into_any_element()
                    } else if downloaded {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                IconButton::new(format!("reveal-{asin}"), Icon::Folder, 16.)
                                    .hit_area(30.)
                                    .hover_fill(theme_fill)
                                    .tooltip("Show in Finder")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(path) = &local_path
                                            && let Err(error) = std::process::Command::new("open")
                                                .arg("-R")
                                                .arg(path)
                                                .spawn()
                                        {
                                            this.download_notice =
                                                Some(format!("Could not open Finder: {error}"));
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                IconButton::new(button_id, Icon::Play, 16.)
                                    .hit_area(30.)
                                    .color(theme_ink)
                                    .hover_fill(theme_fill)
                                    .tooltip("Play")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.start_player(
                                            key.clone(),
                                            asin.clone(),
                                            download_title.clone(),
                                            cx,
                                        )
                                    })),
                            )
                            .into_any_element()
                    } else if other_download {
                        div()
                            .type_style(ui::text::CAPTION)
                            .text_color(color)
                            .child("One download at a time")
                            .into_any_element()
                    } else {
                        IconButton::new(button_id, Icon::Download, 16.)
                            .hit_area(30.)
                            .color(theme_ink)
                            .hover_fill(theme_fill)
                            .tooltip("Download to this Mac")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.download(key.clone(), asin.clone(), download_title.clone(), cx)
                            }))
                            .into_any_element()
                    })
            }))
            .into_any_element()
    }
}
