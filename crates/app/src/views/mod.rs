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
        match &self.account {
            Account::Loading => base().child(heading("Connecting to Audible"))
                .child(loading_placeholders("Loading your library…", cx)).into_any_element(),
            Account::SignedOut => base().child(heading("Your Audible library, here"))
                .child(copy("Choose your Audible marketplace, then sign in with Amazon. Your device credentials are stored in your Mac’s Keychain.".into()))
                .child(div().flex().gap(px(12.))
                    .child(div().w(px(220.)).child(Select::new(&self.marketplace).placeholder("Choose marketplace").accessibility_label("Audible marketplace")))
                    .child(Button::new("sign-in").label("Sign in with Amazon").variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.begin_sign_in(cx)))))
                .children(self.suggested_marketplace.map(|market| copy(format!("Suggested {} from your Mac’s region. You can choose a different marketplace.", market.display_name()))))
                .children(self.download_notice.iter().map(|message| copy(message.clone())))
                .into_any_element(),
            Account::SigningIn(_) => base().child(heading("Finish signing in"))
                .child(copy("Complete sign-in in your browser. When the final page says it cannot be found, copy its full address and paste it below. Treat this address as private.".into()))
                .child(Input::new(&self.redirect))
                .child(div().flex().gap(px(12.))
                    .child(Button::new("finish").label("Connect account").variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, window, cx| this.finish_sign_in(window, cx))))
                    .child(Button::new("cancel").label("Cancel").on_click(cx.listener(|this, _, _, cx| { this.account = Account::SignedOut; cx.notify(); }))))
                .into_any_element(),
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
