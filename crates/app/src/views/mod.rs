//! Views: one module per screen, plus the window frame. Each adds `Shell`
//! methods that build that screen from the current state.

mod book;
mod browse;
mod explore;
mod frame;
mod home;
pub(crate) mod menus;
pub(crate) mod mini_player;
pub(crate) mod player;
mod settings;

use crate::*;

/// A page heading for empty and status states.
pub(crate) fn heading(title: &'static str) -> gpui_kit::Div {
    div().type_style(ui::text::BOOK_TITLE).child(title)
}

/// Secondary explanatory copy under a heading.
pub(crate) fn copy(text: String, color: gpui_kit::Hsla) -> gpui_kit::Div {
    div()
        .max_w(px(570.))
        .type_style(ui::text::CALLOUT)
        .text_color(color)
        .child(text)
}

/// The narrow column used by status and empty states.
pub(crate) fn page_base() -> gpui_kit::Div {
    div().flex().flex_col().gap(px(18.)).max_w(px(620.))
}

pub(crate) fn loading_placeholders(
    label: impl Into<gpui_kit::SharedString>,
    cx: &Context<Shell>,
) -> gpui_kit::AnyElement {
    let reduced_motion = cx.reduce_motion();
    let fill = cx.theme().colors.fill;
    let label_color = cx.theme().colors.ink_secondary;
    let mut rows = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .w_full()
        .max_w(px(620.))
        .child(
            div()
                .type_style(ui::text::CALLOUT)
                .text_color(label_color)
                .child(ShimmerText::new(label)),
        );
    for index in 0_usize..3 {
        let block = |name: &'static str, width: f32, height: f32| -> gpui_kit::AnyElement {
            if reduced_motion {
                div()
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(7.))
                    .bg(fill)
                    .into_any_element()
            } else {
                div()
                    .id(name)
                    .child(Skeleton::new().w(px(width)).h(px(height)).rounded(px(7.)))
                    .into_any_element()
            }
        };
        rows = rows.child(
            div()
                .id(("loading-row", index))
                .flex()
                .items_center()
                .gap(px(16.))
                .child(block("cover", 60., 60.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(block("title", 180., 14.))
                        .child(block("subtitle", 120., 11.)),
                ),
        );
    }
    rows.into_any_element()
}

impl Shell {
    /// The scrolling content for the current page and account state.
    pub(crate) fn content_view(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if self.page == Page::Settings {
            return self.settings_view(cx);
        }
        let library = match &self.account {
            Account::Ready(library) => library,
            _ => return self.account_status_view(cx),
        };
        match self.page {
            Page::Bookmarks => self.bookmarks_view(library, cx),
            Page::Library if !library.items.is_empty() => self.home_view(library, cx),
            Page::Progress => self.progress_view(library, cx),
            Page::CatalogBook => self.catalog_book_view(library, cx),
            Page::Book => self.book_view(library, cx),
            Page::Explore => self.explore_view(window, cx),
            Page::Authors | Page::Series => self.groups_view(library, cx),
            _ => self.book_list_view(library, cx),
        }
    }

    /// Connecting, signed out, signing in, or a connection error.
    fn account_status_view(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let copy = |text: String| copy(text, color);
        let base = page_base;
        let status = |text: &'static str| div().type_style(ui::text::BODY).text_color(color).child(ShimmerText::new(text));
        match &self.account {
            Account::Loading if self.connecting_after_sign_in => base()
                .child(sign_in_steps(2, cx))
                .child(heading("Connecting to Audible"))
                .child(copy("Signed in. Registering this Mac and loading your library. You can leave this window open.".into()))
                .child(status("Loading your library…"))
                .into_any_element(),
            Account::Loading => base().child(heading("Connecting to Audible"))
                .child(loading_placeholders("Loading your library…", cx)).into_any_element(),
            Account::SignedOut => base()
                .child(sign_in_steps(0, cx))
                .child(heading("Your Audible library, here"))
                .child(copy("Choose your Audible store, then sign in with Amazon in your browser. Your device credentials are stored in your Mac’s Keychain.".into()))
                .child(div().flex().gap(px(12.))
                    .child(div().w(px(220.)).child(Select::new(&self.marketplace).placeholder("Choose store").accessibility_label("Audible store")))
                    .child(Button::new("sign-in").label("Sign in with Amazon").variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.begin_sign_in(cx)))))
                .children(self.suggested_marketplace.map(|market| copy(format!("Suggested {} from your Mac’s region. Use the store you bought your books in.", market.display_name()))))
                .children(self.download_notice.iter().map(|message| copy(message.clone())))
                .into_any_element(),
            Account::SigningIn(_) => {
                let note = self.sign_in_note.clone().map(|(text, is_error)| {
                    let note_color = if is_error { cx.theme().accent.text } else { color };
                    div().flex().items_start().gap(px(8.)).max_w(px(570.))
                        .when(is_error, |row| row.child(ui::icon(Icon::Cancel).size(px(14.)).mt(px(1.)).text_color(note_color)))
                        .child(div().type_style(ui::text::BODY).text_color(note_color).child(text))
                });
                base()
                    .child(sign_in_steps(1, cx))
                    .child(heading("Finish signing in"))
                    .child(copy("Sign in with Amazon in your browser. This window continues by itself when you’re done.".into()))
                    .child(note.map_or_else(|| status("Waiting for Amazon…").into_any_element(), IntoElement::into_any_element))
                    .child(div().flex().gap(px(12.))
                        .child(Button::new("reopen").label("Open Amazon again").on_click(cx.listener(|this, _, _, cx| this.reopen_sign_in(cx))))
                        .child(Button::new("cancel").label("Cancel").on_click(cx.listener(|this, _, _, cx| this.cancel_sign_in(cx)))))
                    .into_any_element()
            }
            Account::Error(error) => base().child(heading("Could not connect"))
                .child(copy(error.clone()))
                .child(div().flex().gap(px(12.))
                    .child(Button::new("retry").label("Try loading again").on_click(cx.listener(|this, _, _, cx| this.restore(cx))))
                    .child(Button::new("restart").label("Sign in again").on_click(cx.listener(|this, _, _, cx| { this.account = Account::SignedOut; cx.notify(); }))))
                .into_any_element(),
            Account::Ready(_) => div().into_any_element(),
        }
    }
}

/// Choose store → Sign in → Load library, ticking off the steps before
/// `current`.
fn sign_in_steps(current: usize, cx: &Context<Shell>) -> gpui_kit::Div {
    let colors = cx.theme().colors;
    let done_color: gpui_kit::Hsla = gpui_kit::rgb(0x3b8a4a).into();
    let mut row = div().flex().items_center().gap(px(10.)).mb(px(6.));
    for (index, label) in ["Choose store", "Sign in", "Load library"].into_iter().enumerate() {
        if index > 0 {
            let line = if index <= current { done_color } else { colors.hairline };
            row = row.child(div().h(px(1.)).w(px(32.)).bg(line));
        }
        let marker = div().flex().items_center().justify_center().size(px(18.)).rounded_full();
        let marker = if index < current {
            marker.bg(done_color).child(ui::icon(Icon::Tick).size(px(10.)).text_color(colors.window))
        } else {
            let ink = if index == current { colors.ink } else { colors.ink_secondary };
            marker.border_1().border_color(if index == current { colors.ink } else { colors.hairline })
                .child(div().type_style(ui::text::FOOTNOTE_STRONG).text_color(ink).child((index + 1).to_string()))
        };
        let style = if index == current { ui::text::CAPTION_MEDIUM } else { ui::text::CAPTION };
        let text = if index == current { colors.ink } else { colors.ink_secondary };
        row = row.child(marker).child(div().type_style(style).text_color(text).child(label));
    }
    row
}
