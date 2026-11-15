//! Better Audible's native application shell.

use std::borrow::Cow;

use gpui_kit::{
    App, AppContext, AssetSource, Bounds, Context, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, WindowBounds, WindowOptions,
    div, point, px, size,
};
use ui::{
    ActiveTheme, Elevation, ElevationExt, Icon, ThemeSettings,
    components::{Cover, CoverSize, SidebarHeader, SidebarItem, Toolbar},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Library,
    Progress,
    Explore,
}

struct Shell {
    page: Page,
}

#[derive(Clone, Copy)]
struct Book {
    title: &'static str,
    author: &'static str,
    cover: &'static str,
    remaining: &'static str,
}

const BOOKS: [Book; 6] = [
    Book {
        title: "The Long Winter Light",
        author: "Clara Ellison",
        cover: "long-winter-light",
        remaining: "9 hr 12 min left",
    },
    Book {
        title: "The Quiet Engine",
        author: "Mara Voss",
        cover: "quiet-engine",
        remaining: "4 hr 38 min left",
    },
    Book {
        title: "The Cartographer’s Daughter",
        author: "Nina Hart",
        cover: "cartographers-daughter",
        remaining: "6 hr 20 min left",
    },
    Book {
        title: "Paper Harbours",
        author: "Elliot Vale",
        cover: "paper-harbours",
        remaining: "3 hr 44 min left",
    },
    Book {
        title: "The Small Hours",
        author: "Celia North",
        cover: "small-hours",
        remaining: "7 hr 5 min left",
    },
    Book {
        title: "Northbound",
        author: "Rowan Bell",
        cover: "northbound",
        remaining: "5 hr 17 min left",
    },
];

#[derive(Clone, Copy)]
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let cover: Option<&'static [u8]> = match path {
            "covers/long-winter-light.jpg" => Some(include_bytes!(
                "../../../assets/covers/long-winter-light.jpg"
            )),
            "covers/quiet-engine.jpg" => {
                Some(include_bytes!("../../../assets/covers/quiet-engine.jpg"))
            }
            "covers/cartographers-daughter.jpg" => Some(include_bytes!(
                "../../../assets/covers/cartographers-daughter.jpg"
            )),
            "covers/paper-harbours.jpg" => {
                Some(include_bytes!("../../../assets/covers/paper-harbours.jpg"))
            }
            "covers/small-hours.jpg" => {
                Some(include_bytes!("../../../assets/covers/small-hours.jpg"))
            }
            "covers/northbound.jpg" => {
                Some(include_bytes!("../../../assets/covers/northbound.jpg"))
            }
            _ => ui::Assets::get(path),
        };
        Ok(cover.map(Cow::Borrowed))
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ui::Assets::list(path))
    }
}

fn cover(book: Book, side: f32) -> Cover {
    Cover::new(
        format!("covers/{}.jpg", book.cover),
        CoverSize::Square(side),
        7.,
    )
}

impl Shell {
    fn nav(
        &self,
        label: &'static str,
        icon: Icon,
        page: Page,
        cx: &mut Context<Self>,
    ) -> SidebarItem {
        SidebarItem::new(label, icon, label)
            .selected(self.page == page)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
    }

    fn shelf(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .text_size(px(19.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Still listening"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(22.))
                    .children(BOOKS.into_iter().map(|book| {
                        div()
                            .w(px(145.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(cover(book, 145.))
                            .child(
                                div()
                                    .mt(px(4.))
                                    .font_family("Literata")
                                    .text_size(px(15.))
                                    .child(book.title),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.colors.ink_secondary)
                                    .child(book.remaining),
                            )
                    })),
            )
    }

    fn content(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let book = BOOKS[0];
        match self.page {
            Page::Library => div().flex().flex_col().gap(px(34.))
                .child(div().flex().items_center().gap(px(32.))
                    .child(cover(book, 210.))
                    .child(div().flex().flex_col().gap(px(12.)).max_w(px(520.))
                        .child(div().text_size(px(12.)).text_color(theme.colors.ink_secondary).child("Continue listening"))
                        .child(div().font_family("Literata").text_size(px(36.)).child(book.title))
                        .child(div().text_size(px(14.)).text_color(theme.colors.ink_secondary).child(book.author))
                        .child(div().font_family("Literata").text_size(px(18.)).child("Nine minutes left in this chapter, and about nine hours in the book."))
                        .child(div().text_size(px(12.)).text_color(theme.colors.ink_secondary).child("Sample library preview"))))
                .child(self.shelf(cx)).into_any_element(),
            Page::Progress => div().flex().flex_col().gap(px(18.))
                .child(div().text_size(px(14.)).text_color(theme.colors.ink_secondary).child("Sample listening progress"))
                .children(BOOKS.into_iter().map(|book| div().flex().items_center().gap(px(20.)).pb(px(16.)).border_b_1().border_color(theme.colors.hairline)
                    .child(cover(book, 72.))
                    .child(div().flex_1().flex().flex_col().gap(px(5.))
                        .child(div().font_family("Literata").text_size(px(19.)).child(book.title))
                        .child(div().text_size(px(13.)).text_color(theme.colors.ink_secondary).child(book.author)))
                    .child(div().text_size(px(13.)).text_color(theme.colors.ink_secondary).child(book.remaining))))
                .into_any_element(),
            Page::Explore => div().flex().flex_col().gap(px(12.)).child(div().font_family("Literata").text_size(px(30.)).child("Find your next listen"))
                .child(div().text_size(px(14.)).text_color(theme.colors.ink_secondary).child("Catalog browsing will be available after an Audible account is connected."))
                .into_any_element(),
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let title = match self.page {
            Page::Library => "Library",
            Page::Progress => "In progress",
            Page::Explore => "Explore",
        };
        div()
            .size_full()
            .flex()
            .bg(theme.colors.window)
            .text_color(theme.colors.ink)
            .font_family("Geist")
            .child(
                div()
                    .w(px(220.))
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .px(px(12.))
                    .pt(px(58.))
                    .border_r_1()
                    .border_color(theme.colors.hairline)
                    .bg(theme.colors.sidebar)
                    .child(self.nav("Explore", Icon::Compass, Page::Explore, cx))
                    .child(div().h(px(24.)))
                    .child(SidebarHeader::new("Your library"))
                    .child(self.nav("Library", Icon::Library, Page::Library, cx))
                    .child(self.nav("In progress", Icon::Clock, Page::Progress, cx))
                    .child(div().flex_1())
                    .child(
                        div()
                            .px(px(10.))
                            .pb(px(25.))
                            .text_size(px(12.))
                            .text_color(theme.colors.ink_secondary)
                            .child("Preview · No account connected"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(Toolbar::new(title))
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .overflow_y_scroll()
                            .px(px(36.))
                            .pt(px(28.))
                            .pb(px(36.))
                            .child(self.content(cx)),
                    )
                    .child(
                        div()
                            .mx(px(24.))
                            .mb(px(20.))
                            .h(px(76.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .px(px(16.))
                            .rounded(px(18.))
                            .elevation(Elevation::FloatingPlayer, &theme, window)
                            .child(cover(BOOKS[0], 48.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(div().text_size(px(13.)).child(BOOKS[0].title))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(theme.colors.ink_secondary)
                                            .child("Playback is not connected yet"),
                                    ),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.colors.ink_secondary)
                                    .child("Preview"),
                            ),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application().with_assets(Assets).run(|cx| {
        gpui_kit::init(cx);
        if let Err(error) = ui::init(
            ThemeSettings {
                mode: ui::theme::Mode::Light,
                ..Default::default()
            },
            cx,
        ) {
            eprintln!("Could not initialize the interface: {error:#}");
            cx.quit();
            return;
        }
        let bounds = Bounds::new(point(px(100.), px(100.)), size(px(1200.), px(800.)));
        match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Shell {
                    page: Page::Library,
                })
            },
        ) {
            Ok(_) => cx.activate(true),
            Err(error) => {
                eprintln!("Could not open the app window: {error:#}");
                cx.quit();
            }
        }
    });
}
