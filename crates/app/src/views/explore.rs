//! Explore: featured title, genres, new releases and catalog search results.
use crate::*;

impl Shell {
    pub(crate) fn explore_view(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let hairline = cx.theme().colors.hairline;
        let hover_fill = cx.theme().colors.fill;
        let mut view = div().flex().flex_col().gap(px(36.));
        if self.showing_search {
            view = view
                .child(
                    div()
                        .flex()
                        .child(
                            BackLink::new("explore-home", "Explore").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.showing_search = false;
                                    this.search_message = None;
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .child(if self.searching {
                    loading_placeholders(self.search_message.clone().unwrap_or_default(), cx)
                } else {
                    div()
                        .type_style(ui::text::BODY)
                        .text_color(color)
                        .children(self.search_message.clone())
                        .into_any_element()
                })
                .children(self.search_results.iter().map(|item| {
                    let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                    let image = self.covers.get(&item.asin).cloned();
                    let product = item.clone();
                    div()
                        .id(format!("catalog-{}", item.asin))
                        .cursor_pointer()
                        .rounded(px(8.))
                        .hover(|style| style.bg(hover_fill))
                        .active(|style| style.opacity(0.8))
                        .flex()
                        .items_center()
                        .gap(px(18.))
                        .pb(px(16.))
                        .border_b_1()
                        .border_color(hairline)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_catalog_book(product.clone(), cx)
                        }))
                        .child(
                            Cover::new(
                                image
                                    .map(gpui_kit::ImageSource::from)
                                    .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                CoverSize::Square(72.),
                                ui::metrics::radius::COVER,
                            )
                            .elevation(Elevation::CoverThumb),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(div().type_style(ui::text::BOOK_ROW).child(title))
                                .child(
                                    div()
                                        .type_style(ui::text::BODY)
                                        .text_color(color)
                                        .child(item.author_names()),
                                ),
                        )
                }));
        } else if let Some(feed) = &self.explore_feed {
            if let Some(message) = &feed.warning {
                view = view.child(
                    div()
                        .type_style(ui::text::BODY)
                        .text_color(color)
                        .child(message.clone()),
                );
            }
            if let Some(item) = feed.new_releases.first() {
                let product = item.clone();
                let palette = self.palettes.get(&item.asin).copied().unwrap_or_default();
                let image = self.covers.get(&item.asin).cloned();
                view = view.child(
                    div()
                        .rounded(px(ui::metrics::radius::FLOATING))
                        .p(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(28.))
                        .elevation(Elevation::Banner(palette), cx.theme(), window)
                        .child(
                            Cover::new(
                                image
                                    .map(gpui_kit::ImageSource::from)
                                    .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                CoverSize::Square(168.),
                                ui::metrics::radius::COVER_LARGE,
                            )
                            .elevation(Elevation::CoverOnField),
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
                                        .text_color(palette.on_field_muted)
                                        .child("New release"),
                                )
                                .child(
                                    div()
                                        .mt(px(6.))
                                        .type_style(ui::text::BOOK_BANNER)
                                        .text_color(palette.on_field)
                                        .child(
                                            item.title.clone().unwrap_or_else(|| "Untitled".into()),
                                        ),
                                )
                                .child(
                                    div()
                                        .mt(px(6.))
                                        .type_style(ui::text::CALLOUT)
                                        .text_color(palette.on_field_muted)
                                        .child(item.author_names()),
                                )
                                .child(
                                    div().mt(px(20.)).flex().child(
                                        Button::new("featured-book")
                                            .label("View book")
                                            .variant(ButtonVariant::Light(palette))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.open_catalog_book(product.clone(), cx)
                                            })),
                                    ),
                                ),
                        ),
                );
            }
            if !feed.genres.is_empty() {
                view = view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .child(SectionHeader::new("genres-header", "Browse by genre"))
                        .child(
                            div().flex().gap(px(10.)).children(
                                feed.genres
                                    .iter()
                                    .take(6)
                                    .enumerate()
                                    .map(|(index, genre)| {
                                        let id = genre.id.clone();
                                        let name = genre.name.clone();
                                        let search_name = name.clone();
                                        let accent = ui::theme::Accent::resolve(
                                            ui::theme::AccentTheme::ALL
                                                [index % ui::theme::AccentTheme::ALL.len()],
                                            cx.theme().appearance,
                                            cx.theme().colors.window,
                                        );
                                        div()
                                            .id(("genre", index))
                                            .cursor_pointer()
                                            .flex_1()
                                            .min_w_0()
                                            .h(px(54.))
                                            .px(px(14.))
                                            .flex()
                                            .items_center()
                                            .rounded(px(ui::metrics::radius::TILE))
                                            .bg(accent.tint)
                                            .text_color(accent.on_tint)
                                            .type_style(ui::text::BODY_MEDIUM)
                                            .hover(|style| style.opacity(0.84))
                                            .active(|style| style.opacity(0.72))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.browse_genre(
                                                    id.clone(),
                                                    search_name.clone(),
                                                    cx,
                                                )
                                            }))
                                            .child(div().w_full().min_w_0().truncate().child(name))
                                    }),
                            ),
                        ),
                );
            }
            if !feed.new_releases.is_empty() {
                view = view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .child(SectionHeader::new("releases-header", "New and noteworthy"))
                        .child(div().flex().flex_wrap().gap(px(20.)).children(
                            feed.new_releases.iter().take(6).map(|item| {
                                let product = item.clone();
                                let image = self.covers.get(&item.asin).cloned();
                                div()
                                    .id(format!("release-{}", item.asin))
                                    .w(px(160.))
                                    .flex_none()
                                    .cursor_pointer()
                                    .hover(|style| style.opacity(0.86))
                                    .active(|style| style.opacity(0.72))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_catalog_book(product.clone(), cx)
                                    }))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        Cover::new(
                                            image
                                                .map(gpui_kit::ImageSource::from)
                                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                            CoverSize::Square(160.),
                                            ui::metrics::radius::COVER,
                                        )
                                        .elevation(Elevation::CoverShelf),
                                    )
                                    .child(
                                        div()
                                            .mt(px(10.))
                                            .truncate()
                                            .type_style(ui::text::CAPTION_MEDIUM)
                                            .child(
                                                item.title
                                                    .clone()
                                                    .unwrap_or_else(|| "Untitled".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(2.))
                                            .truncate()
                                            .type_style(ui::text::FOOTNOTE)
                                            .text_color(color)
                                            .child(item.author_names()),
                                    )
                            }),
                        )),
                );
            }
            if feed.warning.is_some() {
                view = view.child(Button::new("retry-explore").label("Try again").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.explore_feed = None;
                        this.load_explore(cx);
                        cx.notify();
                    }),
                ));
            }
        } else {
            if self.explore_loading {
                view = view.child(loading_placeholders("Loading Explore…", cx));
            } else {
                view = view
                    .child(
                        div()
                            .type_style(ui::text::BODY)
                            .text_color(color)
                            .child("Explore is unavailable right now."),
                    )
                    .child(
                        div().flex().child(
                            Button::new("load-explore")
                                .label("Try again")
                                .on_click(cx.listener(|this, _, _, cx| this.load_explore(cx))),
                        ),
                    );
            }
        }
        view.into_any_element()
    }
}
