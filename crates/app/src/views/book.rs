//! Book pages for library and catalog titles.
use crate::*;

impl Shell {
    /// A book page: back link, hero (cover, title, credit, optional sentence,
    /// actions), then the publisher's blurb and a short list of facts.
    pub(crate) fn book_detail(
        &self,
        back_id: &'static str,
        product: &audible::models::Product,
        sentence: Option<String>,
        actions: gpui_kit::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.colors.ink_secondary;
        let title = product.title.clone().unwrap_or_else(|| "Untitled".into());
        let narrator = product
            .narrators
            .iter()
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let author = product
            .authors
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let credit = if narrator.is_empty() {
            author
        } else {
            format!("{author}, read by {narrator}")
        };
        let cover = self.covers.get(&product.asin).cloned();
        let blurb = product
            .merchandising_summary
            .as_deref()
            .or(product.publisher_summary.as_deref())
            .map(blurb_paragraphs)
            .filter(|paragraphs| !paragraphs.is_empty());
        let mut facts: Vec<(&'static str, String)> = Vec::new();
        if let Some(minutes) = product.runtime_length_min {
            facts.push((
                "Length",
                model::format::hours_minutes(std::time::Duration::from_secs(
                    u64::from(minutes) * 60,
                )),
            ));
        }
        if let Some(series) = product.series.first().and_then(|s| s.title.clone()) {
            facts.push(("Series", series));
        }
        if let Some(date) = product.release_date.clone() {
            // Audible sends ISO dates; show them the way people say them.
            let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .map(|d| d.format("%-d %B %Y").to_string())
                .unwrap_or(date);
            facts.push(("Released", date));
        }
        if let Some(publisher) = product.publisher_name.clone() {
            facts.push(("Publisher", publisher));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(
                div().flex().child(
                    BackLink::new(back_id, self.back_label())
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(36.))
                    .child(
                        Cover::new(
                            cover
                                .map(gpui_kit::ImageSource::from)
                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                            CoverSize::Square(220.),
                            ui::metrics::radius::COVER_LARGE,
                        )
                        .elevation(Elevation::CoverHero),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().type_style(ui::text::BOOK_DISPLAY).child(title))
                            .children(product.subtitle.clone().map(|subtitle| {
                                // The display title's descenders need room.
                                div()
                                    .mt(px(12.))
                                    .type_style(ui::text::LEAD)
                                    .text_color(muted)
                                    .child(subtitle)
                            }))
                            .child(
                                div()
                                    .mt(px(8.))
                                    .type_style(ui::text::CALLOUT)
                                    .text_color(muted)
                                    .child(credit),
                            )
                            .children(sentence.map(|sentence| {
                                div()
                                    .mt(px(18.))
                                    .type_style(ui::text::BOOK_SENTENCE)
                                    .child(sentence)
                            }))
                            .child(div().mt(px(20.)).flex().child(actions)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(48.))
                    .children(blurb.map(|paragraphs| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(620.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .type_style(ui::text::BLURB)
                            .children(paragraphs.into_iter().map(|paragraph| div().child(paragraph)))
                    }))
                    .when(!facts.is_empty(), |el: gpui_kit::Div| {
                        el.child(div().w(px(240.)).flex_none().flex().flex_col().children(
                            facts.into_iter().map(|(label, value)| {
                                div()
                                    .flex()
                                    .justify_between()
                                    .gap(px(16.))
                                    .py(px(9.))
                                    .border_b_1()
                                    .border_color(theme.colors.hairline)
                                    .type_style(ui::text::BODY)
                                    .child(div().text_color(muted).child(label))
                                    .child(div().min_w_0().truncate().child(value))
                            }),
                        ))
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn catalog_book_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let base = page_base;
        let Some(item) = &self.selected_catalog_book else {
            return base().child(heading("Book not found")).into_any_element();
        };
        let url = Marketplace::ALL
            .iter()
            .find(|market| market.display_name() == library.marketplace)
            .map(|market| format!("https://www.audible.{}/pd/{}", market.domain(), item.asin));
        let asin = item.asin.clone();
        let waiting = self.pending_purchase.as_ref() == Some(&item.asin);
        let muted = cx.theme().colors.ink_secondary;
        let actions = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .children(url.map(|url| {
                        Button::new("buy-on-audible")
                            .label("Buy on Audible")
                            .icon(Icon::External)
                            .variant(ButtonVariant::Primary)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.open_url(&url);
                                this.pending_purchase = Some(asin.clone());
                                cx.notify();
                            }))
                    }))
                    .children(waiting.then(|| {
                        Button::new("check-purchase")
                            .label(if self.refreshing {
                                "Checking…"
                            } else {
                                "I bought it"
                            })
                            .icon(Icon::Refresh)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.library_fetched = None;
                                this.refresh_library(cx);
                            }))
                    })),
            )
            .children(waiting.then(|| {
                div().type_style(ui::text::CAPTION).text_color(muted).child(
                    "Once you buy it, come back here. The book moves to your library on its own.",
                )
            }));
        self.book_detail(
            "catalog-book-back",
            item,
            None,
            actions.into_any_element(),
            cx,
        )
    }

    pub(crate) fn book_view(
        &self,
        library: &backend::Library,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let base = page_base;
        let Some(item) = self
            .selected_book
            .as_ref()
            .and_then(|asin| library.items.iter().find(|item| &item.asin == asin))
        else {
            return base()
                .child(heading("Book not found"))
                .child(
                    BackLink::new("book-back-missing", "Library")
                        .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                )
                .into_any_element();
        };
        let local_path = backend::downloaded_path(&library.account_key, &item.asin);
        let sentence = if book_finished(item) {
            Some("You finished this book.".to_string())
        } else {
            item.listening_status
                .as_ref()
                .and_then(|status| status.time_remaining_seconds)
                .filter(|_| book_progress(item).unwrap_or(0.) > 0.)
                .map(|seconds| {
                    format!(
                        "{}.",
                        model::format::book_left_long(std::time::Duration::from_secs(seconds))
                    )
                })
        };
        let actions = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(self.resume_action("book", &library.account_key, item, true, cx))
            .children(local_path.map(|path| {
                Button::new("book-reveal")
                    .label("Show in Finder")
                    .icon(Icon::Folder)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Err(error) = std::process::Command::new("open")
                            .arg("-R")
                            .arg(&path)
                            .spawn()
                        {
                            this.download_notice = Some(format!("Could not open Finder: {error}"));
                            cx.notify();
                        }
                    }))
            }));
        self.book_detail("book-back", item, sentence, actions.into_any_element(), cx)
    }
}
