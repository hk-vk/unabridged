//! Better Audible's native application shell.
mod backend;
mod mini_player;
mod player;
mod region;
mod settings;

use audible::{Marketplace, auth::LoginSession};
use gpui_kit::{
    AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
    component::{
        Root,
        input::{Input, InputEvent, InputState},
        select::{SearchableVec, Select, SelectState},
        shimmer::ShimmerText,
        skeleton::Skeleton,
    },
    div, linear_color_stop, linear_gradient, point,
    prelude::FluentBuilder as _,
    px, relative, size,
};
use ui::{
    ActiveTheme, Elevation, ElevationExt, Icon, TypographyExt,
    components::{
        BackLink, Button, ButtonShape, ButtonVariant, Chip, ChipStyle, Cover, CoverSize,
        IconButton, Scrubber, ScrubberColors, SearchField, SectionHeader, SegmentedProgress,
        SidebarHeader, SidebarItem, Toolbar,
    },
};

fn loading_placeholders(label: &'static str, cx: &Context<Shell>) -> gpui_kit::AnyElement {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Library,
    AllBooks,
    Progress,
    Explore,
    OnThisMac,
    Bookmarks,
    Search,
    Finished,
    Authors,
    Series,
    AuthorBooks,
    SeriesBooks,
    Book,
    CatalogBook,
    NowPlaying,
    Settings,
}

/// Plain text from Audible's HTML blurbs: tags dropped, entities decoded,
/// whitespace collapsed.
fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The name a back link shows for `page`.
fn page_name(page: Page) -> &'static str {
    match page {
        Page::Progress => "In progress",
        Page::Explore | Page::Search => "Explore",
        Page::OnThisMac => "On this Mac",
        Page::Bookmarks => "Bookmarks",
        Page::Finished => "Finished",
        Page::Authors | Page::AuthorBooks => "Authors",
        Page::Series | Page::SeriesBooks => "Series",
        Page::Book | Page::CatalogBook => "Book",
        Page::AllBooks => "All books",
        Page::Settings => "Settings",
        Page::Library | Page::NowPlaying => "Library",
    }
}

enum Account {
    Loading,
    SignedOut,
    SigningIn(Box<LoginSession>),
    Ready(backend::Library),
    Error(String),
}
struct ActiveDownload {
    asin: audible::Asin,
    title: String,
    event: std::sync::Arc<std::sync::Mutex<audible::DownloadEvent>>,
}

fn download_display(event: audible::DownloadEvent) -> (&'static str, String, Option<f32>) {
    fn megabytes(bytes: u64) -> f64 {
        bytes as f64 / 1_000_000.0
    }
    match event {
        audible::DownloadEvent::Licensing => (
            "Preparing download",
            "Requesting the book from Audible".into(),
            None,
        ),
        audible::DownloadEvent::Downloading(progress) => {
            let amount = match progress.total {
                Some(total) if total > 0 => {
                    format!(
                        "{:.1} of {:.1} MB",
                        megabytes(progress.downloaded),
                        megabytes(total)
                    )
                }
                _ => format!("{:.1} MB downloaded", megabytes(progress.downloaded)),
            };
            let fraction = progress.fraction().map(|value| value as f32);
            let detail = fraction.map_or(amount.clone(), |value| {
                format!("{:.0}% · {amount}", value * 100.)
            });
            ("Downloading", detail, fraction)
        }
        audible::DownloadEvent::Decrypting(progress) => {
            let fraction = progress.fraction().map(|value| value as f32);
            let detail = fraction.map_or_else(
                || "Finishing the audio file".into(),
                |value| format!("{:.0}% · Finishing the audio file", value * 100.),
            );
            ("Preparing audio", detail, fraction)
        }
    }
}

fn book_finished(item: &audible::models::LibraryItem) -> bool {
    item.is_finished
        .or(item
            .listening_status
            .as_ref()
            .and_then(|status| status.is_finished))
        .unwrap_or(false)
}

fn book_progress(item: &audible::models::LibraryItem) -> Option<f64> {
    item.percent_complete
        .or(item
            .listening_status
            .as_ref()
            .and_then(|status| status.percent_complete))
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0., 100.))
}

fn current_chapter(
    chapters: &[audible::models::Chapter],
    position_ms: u64,
) -> Option<(usize, &audible::models::Chapter)> {
    chapters.iter().enumerate().rev().find(|(_, chapter)| {
        chapter.length_ms > 0
            && position_ms >= chapter.start_offset_ms
            && position_ms < chapter.start_offset_ms.saturating_add(chapter.length_ms)
    })
}

#[derive(Clone)]
enum SleepTimer {
    Off,
    Deadline(std::time::Instant),
    EndOfChapter {
        asin: audible::Asin,
        end_seconds: f64,
    },
}

impl SleepTimer {
    fn due(&self, now: std::time::Instant, current: Option<(&audible::Asin, f64)>) -> bool {
        match self {
            Self::Off => false,
            Self::Deadline(deadline) => now >= *deadline,
            Self::EndOfChapter { asin, end_seconds } => {
                current.is_some_and(|(current_asin, position)| {
                    current_asin == asin && position >= *end_seconds
                })
            }
        }
    }
    fn label(&self, now: std::time::Instant) -> String {
        match self {
            Self::Off => "Sleep timer off".into(),
            Self::Deadline(deadline) => format!(
                "Stop in {} min",
                deadline
                    .saturating_duration_since(now)
                    .as_secs()
                    .div_ceil(60)
            ),
            Self::EndOfChapter { .. } => "Stop after this chapter".into(),
        }
    }

    /// Text for the sleep chip: "Sleep" when off, otherwise the countdown.
    fn chip_label(&self, now: std::time::Instant) -> String {
        match self {
            Self::Off => "Sleep".into(),
            _ => self.label(now),
        }
    }
}

struct ActivePlayer {
    asin: audible::Asin,
    account_key: String,
    title: String,
    player: player::Player,
    was_playing: bool,
    chapters: Vec<audible::models::Chapter>,
    chapters_loading: bool,
}

struct Shell {
    page: Page,
    main_window: Option<gpui_kit::AnyWindowHandle>,
    mini_window: Option<gpui_kit::AnyWindowHandle>,
    /// The mini player shows time left in the book instead of the chapter.
    mini_book_time: bool,
    mini_opening: bool,
    main_opening: bool,
    previous_page: Page,
    selected_book: Option<audible::Asin>,
    selected_catalog_book: Option<audible::models::Product>,
    seek_generation: u64,
    navigation_generation: u64,
    chapters_open: bool,
    bookmarks: Vec<(audible::Asin, u64)>,
    bookmarks_loading: bool,
    sleep: SleepTimer,
    account: Account,
    marketplace: Entity<SelectState<SearchableVec<&'static str>>>,
    suggested_marketplace: Option<Marketplace>,
    redirect: Entity<InputState>,
    search: Entity<InputState>,
    library_search: Entity<InputState>,
    browse_name: Option<String>,
    _subscriptions: Vec<gpui_kit::Subscription>,
    search_results: Vec<audible::models::Product>,
    showing_search: bool,
    explore_feed: Option<backend::ExploreFeed>,
    explore_loading: bool,
    searching: bool,
    search_message: Option<String>,
    download_notice: Option<String>,
    download: Option<ActiveDownload>,
    player: Option<ActivePlayer>,
    loading_player: bool,
    covers: std::collections::HashMap<audible::Asin, std::path::PathBuf>,
    covers_pending: std::collections::HashSet<audible::Asin>,
    palettes: std::collections::HashMap<audible::Asin, ui::theme::BookPalette>,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let redirect =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste the final URL here"));
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search the Audible catalog"));
        let library_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search titles, authors or narrators")
        });
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, window, cx| {
                ui::Theme::sync_system_appearance(window.appearance(), cx)
            }),
            cx.subscribe(
                &library_search,
                |this: &mut Self, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        // Typing in the toolbar field shows results; clearing it
                        // returns to the library.
                        let empty = input.read(cx).value().trim().is_empty();
                        if !empty && this.page != Page::Search {
                            this.page = Page::Search;
                        } else if empty && this.page == Page::Search {
                            this.page = Page::Library;
                        }
                        cx.notify();
                    }
                },
            ),
            cx.subscribe(&search, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.search_catalog(cx);
                }
            }),
        ];
        let marketplaces = SearchableVec::new(
            Marketplace::ALL
                .iter()
                .map(|m| m.display_name())
                .collect::<Vec<_>>(),
        );
        let suggested_marketplace = region::suggested_marketplace();
        let selected = suggested_marketplace
            .and_then(|suggestion| {
                Marketplace::ALL
                    .iter()
                    .position(|market| *market == suggestion)
            })
            .map(gpui_kit::component::IndexPath::new);
        let marketplace = cx.new(|cx| SelectState::new(marketplaces, selected, window, cx));
        let mut shell = Self {
            page: Page::Library,
            main_window: Some(window.window_handle()),
            mini_window: None,
            mini_book_time: false,
            mini_opening: false,
            main_opening: false,
            previous_page: Page::Library,
            selected_book: None,
            selected_catalog_book: None,
            seek_generation: 0,
            navigation_generation: 0,
            chapters_open: false,
            bookmarks: Vec::new(),
            bookmarks_loading: false,
            sleep: SleepTimer::Off,
            account: Account::Loading,
            marketplace,
            suggested_marketplace,
            redirect,
            search,
            library_search,
            browse_name: None,
            _subscriptions: subscriptions,
            search_results: Vec::new(),
            showing_search: false,
            explore_feed: None,
            explore_loading: false,
            searching: false,
            search_message: None,
            download_notice: None,
            download: None,
            player: None,
            loading_player: false,
            covers: Default::default(),
            covers_pending: Default::default(),
            palettes: Default::default(),
        };
        shell.restore(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.download.is_some() {
                            cx.notify();
                        }
                        let due = this.sleep.due(
                            std::time::Instant::now(),
                            this.player
                                .as_ref()
                                .map(|active| (&active.asin, active.player.position())),
                        );
                        if due {
                            if let Some(active) = &mut this.player {
                                active.player.pause();
                                active.was_playing = false;
                            }
                            this.sleep = SleepTimer::Off;
                            this.sync_position(cx);
                            this.download_notice = Some("Sleep timer stopped playback.".into());
                            cx.notify();
                        }
                        let ended = this
                            .player
                            .as_ref()
                            .is_some_and(|p| p.was_playing && !p.player.is_playing());
                        if ended {
                            if let Some(player) = &mut this.player {
                                player.was_playing = false;
                            }
                            this.sync_position(cx);
                            cx.notify();
                        }
                        if this.player.as_ref().is_some_and(|p| p.player.is_playing()) {
                            ticks += 1;
                            if ticks >= 150 {
                                this.sync_position(cx);
                                ticks = 0;
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        shell
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        self.account = Account::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { backend::restore() }).await;
            let _ = this.update(cx, |this, cx| {
                this.account = match result {
                    Ok(Some(library)) => Account::Ready(library),
                    Ok(None) => Account::SignedOut,
                    Err(error) => Account::Error(error.to_string()),
                };
                this.load_covers(cx);
                if this.page == Page::Explore {
                    this.load_explore(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn begin_sign_in(&mut self, cx: &mut Context<Self>) {
        let selected = self.marketplace.read(cx).selected_value().copied();
        let Some(marketplace) = Marketplace::ALL
            .into_iter()
            .find(|m| Some(m.display_name()) == selected)
        else {
            self.download_notice = Some("Choose your Audible marketplace to continue.".into());
            cx.notify();
            return;
        };
        self.download_notice = None;
        let session = LoginSession::new(marketplace);
        match session.authorization_url() {
            Ok(url) => {
                cx.open_url(url.as_str());
                self.account = Account::SigningIn(Box::new(session));
            }
            Err(error) => self.account = Account::Error(error.to_string()),
        }
        cx.notify();
    }

    fn finish_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Account::SigningIn(session) = &self.account else {
            return;
        };
        let redirect = self.redirect.read(cx).value().to_string();
        self.redirect
            .update(cx, |input, cx| input.set_value("", window, cx));
        let valid_host = reqwest::Url::parse(redirect.trim())
            .ok()
            .is_some_and(|url| {
                url.scheme() == "https"
                    && url.host_str()
                        == Some(format!("www.amazon.{}", session.marketplace().domain()).as_str())
            });
        if !valid_host || !LoginSession::is_redirect(&redirect) {
            self.account = Account::Error("That URL does not contain the Audible sign-in redirect. Start sign-in again and copy the entire final URL.".into());
            cx.notify();
            return;
        }
        let session = (**session).clone();
        self.account = Account::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::sign_in(session, redirect) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.account = match result {
                    Ok(library) => Account::Ready(library),
                    Err(error) => Account::Error(error.to_string()),
                };
                this.load_covers(cx);
                if this.page == Page::Explore {
                    this.load_explore(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn search_catalog(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_string();
        if query.is_empty() || self.searching {
            return;
        }
        self.searching = true;
        self.showing_search = true;
        self.search_results.clear();
        self.search_message = Some("Searching Audible…".into());
        cx.notify();
        let display_query = query.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::search_catalog(query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.searching = false;
                match result {
                    Ok(products) => {
                        this.search_message = Some(if products.is_empty() {
                            format!("No results for “{display_query}”.")
                        } else {
                            format!("{} results for “{display_query}”", products.len())
                        });
                        this.search_results = products;
                        this.load_covers(cx);
                    }
                    Err(error) => this.search_message = Some(format!("Search failed: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_explore(&mut self, cx: &mut Context<Self>) {
        if self.explore_loading
            || self.explore_feed.is_some()
            || !matches!(self.account, Account::Ready(_))
        {
            return;
        }
        self.explore_loading = true;
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { backend::explore() }).await;
            let _ = this.update(cx, |this, cx| {
                this.explore_loading = false;
                match result {
                    Ok(feed) => {
                        this.explore_feed = Some(feed);
                        this.load_covers(cx);
                    }
                    Err(error) => {
                        this.search_message = Some(format!("Could not load Explore: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn browse_genre(
        &mut self,
        id: audible::models::CategoryId,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if self.searching {
            return;
        }
        self.showing_search = true;
        self.searching = true;
        self.search_results.clear();
        self.search_message = Some(format!("Loading {name}…"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::browse_genre(id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.searching = false;
                match result {
                    Ok(products) => {
                        this.search_message = Some(if products.is_empty() {
                            format!("No books found in {name}.")
                        } else {
                            format!("{name} · {} books", products.len())
                        });
                        this.search_results = products;
                        this.load_covers(cx);
                    }
                    Err(error) => {
                        this.search_message = Some(format!("Could not load {name}: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_covers(&mut self, cx: &mut Context<Self>) {
        let Account::Ready(library) = &self.account else {
            return;
        };
        let mut covers: Vec<_> = library
            .items
            .iter()
            .filter_map(|item| {
                item.cover_url(500)
                    .map(|url| (item.asin.clone(), url.to_string()))
            })
            .collect();
        if let Some(feed) = &self.explore_feed {
            covers.extend(feed.new_releases.iter().filter_map(|item| {
                item.cover_url(500)
                    .map(|url| (item.asin.clone(), url.to_string()))
            }));
        }
        covers.extend(self.search_results.iter().filter_map(|item| {
            item.cover_url(500)
                .map(|url| (item.asin.clone(), url.to_string()))
        }));
        covers.retain(|(asin, _)| {
            !self.covers.contains_key(asin) && self.covers_pending.insert(asin.clone())
        });
        let queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            covers,
        )));
        for _ in 0..4 {
            let queue = queue.clone();
            cx.spawn(async move |this, cx| {
                loop {
                    let next = queue.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some((asin, url)) = next else { break };
                    let key = asin.clone();
                    let result = cx
                        .background_spawn(async move {
                            let path = backend::cover(&asin, &url)?;
                            let palette = image::ImageReader::open(&path)
                                .ok()
                                .and_then(|reader| reader.with_guessed_format().ok())
                                .and_then(|reader| reader.decode().ok())
                                .and_then(|image| {
                                    let pixels = image.thumbnail(128, 128).to_rgba8();
                                    ui::theme::BookPalette::dominant_color(pixels.as_raw())
                                })
                                .map(ui::theme::BookPalette::from_cover_color);
                            Ok::<_, anyhow::Error>((path, palette))
                        })
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            this.covers_pending.remove(&key);
                            if let Ok((path, palette)) = result {
                                if let Some(palette) = palette {
                                    this.palettes.insert(key.clone(), palette);
                                }
                                this.covers.insert(key, path);
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    fn download(
        &mut self,
        key: String,
        asin: audible::Asin,
        title: String,
        cx: &mut Context<Self>,
    ) {
        if self.download.is_some() {
            return;
        }
        let event = std::sync::Arc::new(std::sync::Mutex::new(audible::DownloadEvent::Licensing));
        self.download = Some(ActiveDownload {
            asin: asin.clone(),
            title: title.clone(),
            event: event.clone(),
        });
        self.download_notice = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::download(&key, asin, event) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.download = None;
                this.download_notice = Some(match result {
                    Ok(_) => format!("{title} is downloaded to this Mac."),
                    Err(error) => format!("Download failed: {error}"),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn load_bookmarks(&mut self, cx: &mut Context<Self>) {
        let Account::Ready(library) = &self.account else {
            return;
        };
        let key = library.account_key.clone();
        let asins = library
            .items
            .iter()
            .map(|item| item.asin.clone())
            .collect::<Vec<_>>();
        self.bookmarks_loading = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::bookmarks(&key, &asins) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.bookmarks_loading = false;
                match result {
                    Ok(bookmarks) => this.bookmarks = bookmarks,
                    Err(error) => {
                        this.download_notice = Some(format!("Could not load bookmarks: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn change_bookmark(
        &mut self,
        asin: audible::Asin,
        position: u64,
        add: bool,
        cx: &mut Context<Self>,
    ) {
        let Account::Ready(library) = &self.account else {
            return;
        };
        let key = library.account_key.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { backend::update_bookmark(&key, &asin, position, add) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                this.download_notice = Some(match result {
                    Ok(()) if add => "Bookmark saved on this Mac.".into(),
                    Ok(()) => "Bookmark removed.".into(),
                    Err(error) => format!("Could not save bookmark: {error}"),
                });
                this.load_bookmarks(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn add_current_bookmark(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &self.player {
            let asin = player.asin.clone();
            let position = (player.player.position().max(0.) * 1000.) as u64;
            self.change_bookmark(asin, position, true, cx);
        }
    }

    fn load_chapters(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let asin = player.asin.clone();
        let key = player.account_key.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let asin = asin.clone();
                    async move { backend::chapters(&key, &asin) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(player) = &mut this.player
                    && player.asin == asin
                {
                    player.chapters_loading = false;
                    if let Ok(info) = result {
                        player.chapters = info
                            .flatten()
                            .into_iter()
                            .filter(|chapter| chapter.chapters.is_empty())
                            .cloned()
                            .collect();
                        player
                            .chapters
                            .sort_by_key(|chapter| chapter.start_offset_ms);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn sync_position(&self, cx: &mut Context<Self>) {
        let Some(active) = &self.player else { return };
        let asin = active.asin.clone();
        let key = active.account_key.clone();
        let millis = (active.player.position().max(0.) * 1000.) as u64;
        let version = backend::register_save(&asin);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(
                    async move { backend::save_position(&key, &asin, millis, version) },
                )
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.download_notice =
                        Some(format!("Could not sync listening position: {error}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_player(
        &mut self,
        key: String,
        asin: audible::Asin,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.start_player_at(key, asin, title, None, cx);
    }

    fn start_player_at(
        &mut self,
        key: String,
        asin: audible::Asin,
        title: String,
        start_at: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if self.loading_player {
            return;
        }
        if self.player.as_ref().is_some_and(|p| p.asin == asin) {
            if let Some(active) = &mut self.player {
                if let Some(position) = start_at {
                    active.player.seek(position as f64 / 1000.);
                }
                if let Err(error) = active.player.play() {
                    self.download_notice = Some(format!("Could not resume playback: {error}"));
                } else {
                    active.was_playing = true;
                }
            }
            self.open_player(cx);
            cx.notify();
            return;
        }
        let Some(path) = backend::downloaded_path(&key, &asin) else {
            return;
        };
        if let Some(active) = &self.player {
            active.player.pause();
            self.sync_position(cx);
        }
        self.loading_player = true;
        self.download_notice = Some(format!("Loading {title}…"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = if let Some(position) = start_at {
                Ok(Some(position))
            } else {
                cx.background_spawn({
                    let asin = asin.clone();
                    let key = key.clone();
                    async move { backend::position(&key, &asin) }
                })
                .await
            };
            let _ = this.update(cx, |this, cx| {
                this.loading_player = false;
                match result.and_then(|position| {
                    let player = player::Player::open(&path)?;
                    if let Some(millis) = position {
                        player.seek(millis as f64 / 1000.);
                    }
                    player.play()?;
                    Ok(player)
                }) {
                    Ok(player) => {
                        let cancelled_timer = matches!(&this.sleep, SleepTimer::EndOfChapter { asin: timer_asin, .. } if timer_asin != &asin);
                        if cancelled_timer { this.sleep = SleepTimer::Off; }
                        this.player = Some(ActivePlayer {
                            asin,
                            account_key: key,
                            title,
                            player,
                            was_playing: true,
                            chapters: Vec::new(),
                            chapters_loading: true,
                        });
                        this.chapters_open = false;
                        this.load_chapters(cx);
                        this.download_notice = cancelled_timer.then(|| "Chapter sleep timer turned off when you changed books.".into());
                        this.open_player(cx);
                    }
                    Err(error) => {
                        this.download_notice = Some(format!("Could not start playback: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &mut self.player {
            if active.player.is_playing() {
                active.player.pause();
                active.was_playing = false;
                self.sync_position(cx);
            } else if let Err(error) = active.player.play() {
                self.download_notice = Some(format!("Could not resume playback: {error}"));
            } else {
                active.was_playing = true;
            }
            cx.notify();
        }
    }

    /// Moves to the next chapter (`forward`), or back to the start of this
    /// chapter, or to the previous one when already near its start.
    fn step_chapter(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(active) = &self.player else { return };
        let duration = active.player.duration();
        let position_ms = (active.player.position().max(0.) * 1000.) as u64;
        let Some((index, chapter)) = current_chapter(&active.chapters, position_ms) else {
            return;
        };
        let target = if forward {
            active.chapters.get(index + 1)
        } else if position_ms.saturating_sub(chapter.start_offset_ms) > 3_000 || index == 0 {
            Some(chapter)
        } else {
            active.chapters.get(index - 1)
        };
        if let Some(target) = target
            && duration > 0.
        {
            let fraction = (target.start_offset_ms as f64 / 1000. / duration) as f32;
            self.seek_fraction(fraction, cx);
        }
    }

    fn close_mini_player(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.mini_window.take() {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
        cx.notify();
    }

    fn skip(&mut self, seconds: f64, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            active.player.seek(active.player.position() + seconds);
            self.sync_position(cx);
            cx.notify();
        }
    }

    fn open_book(&mut self, asin: audible::Asin, cx: &mut Context<Self>) {
        self.selected_book = Some(asin);
        if self.page != Page::Book {
            self.previous_page = self.page;
        }
        self.page = Page::Book;
        self.navigation_generation += 1;
        cx.notify();
    }

    fn open_catalog_book(&mut self, product: audible::models::Product, cx: &mut Context<Self>) {
        if let Account::Ready(library) = &self.account
            && library.items.iter().any(|item| item.asin == product.asin)
        {
            self.open_book(product.asin, cx);
            return;
        }
        self.selected_catalog_book = Some(product);
        self.previous_page = Page::Explore;
        self.page = Page::CatalogBook;
        self.navigation_generation += 1;
        cx.notify();
    }

    fn open_mini_player(&mut self, cx: &mut Context<Self>) {
        if self.player.is_none() || self.mini_opening {
            return;
        }
        self.mini_opening = true;
        let shell = cx.entity();
        let existing = self.mini_window;
        // Opening a window can render immediately. Defer it until the Shell
        // entity is no longer mutably borrowed by the click handler.
        cx.defer(move |cx| {
            if let Some(handle) = existing
                && handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok()
            {
                shell.update(cx, |this, cx| {
                    this.mini_opening = false;
                    cx.notify();
                });
                return;
            }
            let bounds = Bounds::new(
                point(px(160.), px(160.)),
                size(px(mini_player::SIZE.0), px(mini_player::SIZE.1)),
            );
            let view_shell = shell.clone();
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        appears_transparent: cfg!(target_os = "macos"),
                        ..Default::default()
                    }),
                    kind: gpui_kit::WindowKind::Floating,
                    window_background: gpui_kit::WindowBackgroundAppearance::Blurred,
                    show: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    focus: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    is_resizable: false,
                    ..Default::default()
                },
                |window, cx| {
                    mini_player::float_everywhere(window);
                    let mini = cx.new(|cx| mini_player::MiniPlayer::new(view_shell, cx));
                    cx.new(|cx| Root::new(mini, window, cx))
                },
            );
            // The panel may not be attached to its view until after creation;
            // apply the native tweaks again now that it is (they are idempotent).
            if let Ok(handle) = &result {
                let _ = handle.update(cx, |_, window, _| mini_player::float_everywhere(window));
            }
            shell.update(cx, |this, cx| {
                this.mini_opening = false;
                match result {
                    Ok(handle) => {
                        this.mini_window = Some(handle.into());
                        #[cfg(debug_assertions)]
                        if std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_some() {
                            eprintln!("Smoke test: mini player window opened");
                        }
                    }
                    Err(error) => {
                        this.download_notice =
                            Some(format!("Could not open the mini player: {error}"))
                    }
                }
                cx.notify();
            });
            #[cfg(debug_assertions)]
            if std::env::var_os("BETTER_AUDIBLE_SMOKE_REOPEN").is_some() {
                let shell = shell.clone();
                cx.defer(move |cx| {
                    let main = shell.read(cx).main_window;
                    if let Some(handle) = main {
                        let _ = handle.update(cx, |_, window, _| window.remove_window());
                    }
                    shell.update(cx, |this, cx| this.show_main_player(cx));
                });
            }
        });
    }

    fn show_main_player(&mut self, cx: &mut Context<Self>) {
        self.open_player(cx);
        if self.main_opening {
            return;
        }
        self.main_opening = true;
        let shell = cx.entity();
        let existing = self.main_window;
        cx.defer(move |cx| {
            if let Some(handle) = existing
                && handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok()
            {
                shell.update(cx, |this, cx| {
                    this.main_opening = false;
                    cx.notify();
                });
                return;
            }
            let bounds = Bounds::new(point(px(100.), px(100.)), size(px(1200.), px(800.)));
            let view_shell = shell.clone();
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    focus: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    titlebar: Some(TitlebarOptions {
                        appears_transparent: cfg!(target_os = "macos"),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Root::new(view_shell, window, cx)),
            );
            shell.update(cx, |this, cx| {
                this.main_opening = false;
                match result {
                    Ok(handle) => {
                        this.main_window = Some(handle.into());
                        #[cfg(debug_assertions)]
                        if std::env::var_os("BETTER_AUDIBLE_SMOKE_REOPEN").is_some() {
                            eprintln!("Smoke test: main player window reopened");
                        }
                    }
                    Err(error) => {
                        this.download_notice =
                            Some(format!("Could not open the main window: {error}"))
                    }
                }
                cx.notify();
            });
        });
    }

    fn open_player(&mut self, cx: &mut Context<Self>) {
        if self.player.is_some() {
            if self.page != Page::NowPlaying {
                self.previous_page = self.page;
            }
            self.page = Page::NowPlaying;
            self.navigation_generation += 1;
            cx.notify();
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        let destination = if self.previous_page == self.page {
            Page::Library
        } else {
            self.previous_page
        };
        self.page = destination;
        self.previous_page = if destination == Page::CatalogBook {
            Page::Explore
        } else {
            Page::Library
        };
        self.navigation_generation += 1;
        cx.notify();
    }

    fn seek_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let Some(active) = &self.player else { return };
        active
            .player
            .seek(active.player.duration() * f64::from(fraction.clamp(0., 1.)));
        let asin = active.asin.clone();
        self.seek_generation += 1;
        let generation = self.seek_generation;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(700))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.seek_generation == generation
                    && this.player.as_ref().is_some_and(|p| p.asin == asin)
                {
                    this.sync_position(cx);
                }
            });
        })
        .detach();
    }

    /// Steps the sleep chip: off, 15 min, 30 min, 1 hour, end of chapter, off.
    fn cycle_sleep(&mut self, cx: &mut Context<Self>) {
        let next = match &self.sleep {
            SleepTimer::Off => "15 minutes",
            SleepTimer::Deadline(deadline) => {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left <= std::time::Duration::from_secs(15 * 60) {
                    "30 minutes"
                } else if left <= std::time::Duration::from_secs(30 * 60) {
                    "1 hour"
                } else {
                    "End of chapter"
                }
            }
            SleepTimer::EndOfChapter { .. } => "Off",
        };
        self.set_sleep(next, cx);
    }

    /// Where the back link goes, named for the destination.
    fn back_label(&self) -> &'static str {
        let destination = if self.previous_page == self.page {
            Page::Library
        } else {
            self.previous_page
        };
        page_name(destination)
    }

    fn set_sleep(&mut self, choice: &str, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        self.sleep = match choice {
            "15 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(15 * 60)),
            "30 minutes" => SleepTimer::Deadline(now + std::time::Duration::from_secs(30 * 60)),
            "1 hour" => SleepTimer::Deadline(now + std::time::Duration::from_secs(60 * 60)),
            "End of chapter" => {
                let chapter = self.player.as_ref().and_then(|active| {
                    current_chapter(
                        &active.chapters,
                        (active.player.position().max(0.) * 1000.) as u64,
                    )
                    .map(|(_, chapter)| {
                        (
                            active.asin.clone(),
                            chapter.start_offset_ms.saturating_add(chapter.length_ms),
                        )
                    })
                });
                if let Some((asin, end_ms)) = chapter {
                    SleepTimer::EndOfChapter {
                        asin,
                        end_seconds: end_ms as f64 / 1000.,
                    }
                } else {
                    self.download_notice = Some(
                        "Chapter timing is unavailable. Choose a timed sleep option instead."
                            .into(),
                    );
                    SleepTimer::Off
                }
            }
            _ => SleepTimer::Off,
        };
        cx.notify();
    }

    fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            active.player.set_volume(volume);
            cx.notify();
        }
    }

    fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = &self.player {
            let speed = active.player.rate();
            active.player.set_rate(
                [0.75, 1.0, 1.25, 1.5, 1.75, 2.0]
                    .into_iter()
                    .find(|next| *next > speed + 0.01)
                    .unwrap_or(0.75),
            );
            cx.notify();
        }
    }

    fn player_view(&self, window: &Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
        let position = active.player.position().max(0.);
        let duration = active.player.duration().max(0.);
        let chapter = current_chapter(&active.chapters, (position * 1000.) as u64);
        let (timeline_start, timeline_end, caption) = if let Some((index, chapter)) = chapter {
            let start = chapter.start_offset_ms as f64 / 1000.;
            let end = (chapter.start_offset_ms.saturating_add(chapter.length_ms) as f64 / 1000.)
                .min(duration);
            let name = if chapter.title.is_empty() {
                format!("Chapter {}", index + 1)
            } else {
                chapter.title.clone()
            };
            (
                start,
                end.max(start),
                format!(
                    "left in {name}, chapter {} of {}",
                    index + 1,
                    active.chapters.len()
                ),
            )
        } else {
            (0., duration, "left in this book".to_string())
        };
        let segment_length = (timeline_end - timeline_start).max(0.);
        let elapsed = (position - timeline_start).clamp(0., segment_length);
        let remaining = (segment_length - elapsed).max(0.) as u64;
        let fraction = if segment_length > 0. {
            (elapsed / segment_length).clamp(0., 1.) as f32
        } else {
            0.
        };
        let time = model::format::clock(std::time::Duration::from_secs(elapsed as u64));
        let book_left = model::format::book_left_long(std::time::Duration::from_secs(
            (duration - position).max(0.) as u64,
        ));
        let left = model::format::clock(std::time::Duration::from_secs(remaining));
        let playing = active.player.is_playing();
        let speed = active.player.rate();
        let title = active.title.clone();
        let weak = cx.entity().downgrade();
        let scrubber = Scrubber::new(
            "full-player-seek",
            fraction,
            ScrubberColors::field(&palette, true),
        )
        .on_seek(move |value, _, cx| {
            let book_fraction = if duration > 0. {
                ((timeline_start + segment_length * f64::from(value)) / duration) as f32
            } else {
                0.
            };
            let _ = weak.update(cx, |this, cx| this.seek_fraction(book_fraction, cx));
        });
        let chapter_panel = if self.chapters_open {
            let message = if active.chapters_loading {
                "Loading chapters…"
            } else {
                "Chapters unavailable for this book."
            };
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
            if active.chapters.is_empty() {
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
                let target = if duration > 0. {
                    (start / duration).clamp(0., 1.) as f32
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

    fn nav(
        &self,
        label: &'static str,
        icon: Icon,
        page: Page,
        cx: &mut Context<Self>,
    ) -> SidebarItem {
        SidebarItem::new(label, icon, label)
            .selected(self.page == page || (page == Page::Explore && self.page == Page::Search))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                if page == Page::Bookmarks {
                    this.load_bookmarks(cx);
                }
                if page == Page::Explore {
                    this.load_explore(cx);
                }
                cx.notify();
            }))
    }

    fn shelf(
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
    fn resume_action(
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

    /// A book page: back link, hero (cover, title, credit, optional sentence,
    /// actions), then the publisher's blurb and a short list of facts.
    fn book_detail(
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
            .map(strip_html)
            .filter(|text| !text.is_empty());
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
                                div()
                                    .mt(px(6.))
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
                    .children(blurb.map(|blurb| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(620.))
                            .type_style(ui::text::BLURB)
                            .child(blurb)
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

    /// One "In progress" row: cover, title, credit and progress bar, then the
    /// percentage in pixel type, time left, and the resume action.
    fn progress_row(
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

    fn account_view(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if self.page == Page::Settings {
            return self.settings_view(cx);
        }
        let color = cx.theme().colors.ink_secondary;
        let theme_ink = cx.theme().colors.ink;
        let theme_fill = cx.theme().colors.fill;
        let hairline = cx.theme().colors.hairline;
        let hover_fill = cx.theme().colors.fill;
        let heading = |title: &'static str| div().type_style(ui::text::BOOK_TITLE).child(title);
        let copy = |text: String| {
            div()
                .max_w(px(570.))
                .type_style(ui::text::CALLOUT)
                .text_color(color)
                .child(text)
        };
        let base = || div().flex().flex_col().gap(px(18.)).max_w(px(620.));
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
            Account::Ready(library) => {
                if self.page == Page::Bookmarks {
                    if self.bookmarks_loading { return loading_placeholders("Loading bookmarks…", cx); }
                    if self.bookmarks.is_empty() { return base().child(heading("No bookmarks yet")).child(copy("Save a moment from the player and find it here. Bookmarks are stored on this Mac.".into())).into_any_element(); }
                    return div().flex().flex_col().gap(px(12.))
                        .child(div().type_style(ui::text::BODY).text_color(color).child("Saved on this Mac"))
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
                            Some(div().flex().items_center().gap(px(16.)).py(px(14.)).border_b_1().border_color(hairline)
                                .child(div().flex_1().flex().flex_col().gap(px(4.))
                                    .child(div().type_style(ui::text::BOOK_ROW).child(title))
                                    .child(div().type_style(ui::text::CAPTION).text_color(color).child(time)))
                                .child(if downloaded {
                                    IconButton::new(format!("bookmark-play-{asin}-{position}"), Icon::Play, 16.).hit_area(30.).color(theme_ink).hover_fill(theme_fill).tooltip("Play from here").on_click(cx.listener(move |this, _, _, cx| this.start_player_at(key.clone(), play_asin.clone(), play_title.clone(), Some(bookmark_position), cx)))
                                } else {
                                    IconButton::new(format!("bookmark-book-{asin}-{position}"), Icon::ChevronRight, 16.).hit_area(30.).hover_fill(theme_fill).tooltip("Open book").on_click(cx.listener(move |this, _, _, cx| this.open_book(play_asin.clone(), cx)))
                                })
                                .child(IconButton::new(format!("bookmark-remove-{asin}-{position}"), Icon::Delete, 16.).hit_area(30.).hover_fill(theme_fill).tooltip("Remove bookmark").on_click(cx.listener(move |this, _, _, cx| this.change_bookmark(remove_asin.clone(), bookmark_position, false, cx)))))
                        })).into_any_element();
                }
                if self.page == Page::Library && !library.items.is_empty() {
                    let listening: Vec<_> = library.items.iter().filter(|item| book_progress(item).unwrap_or(0.) > 0. && !book_finished(item)).collect();
                    let recent: Vec<_> = library.items.iter().take(8).collect();
                    let mut view = div().flex().flex_col().gap(px(40.));
                    if let Some(item) = listening.first() {
                        let details_asin = item.asin.clone();
                        let chapters_asin = item.asin.clone();
                        let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                        let narrator = item.narrator_names();
                        let credit = if narrator.is_empty() { item.author_names() } else { format!("{}, read by {narrator}", item.author_names()) };
                        let cover = self.covers.get(&item.asin).cloned();
                        let sentence = item.listening_status.as_ref().and_then(|status| status.time_remaining_seconds)
                            .map(|seconds| format!("{}.", model::format::book_left_long(std::time::Duration::from_secs(seconds))))
                            .or_else(|| book_progress(item).map(|percent| format!("{percent:.0}% of the way through.")));
                        view = view.child(div().flex().items_center().gap(px(36.))
                            .child(div().id("hero-book").cursor_pointer().active(|style| style.opacity(0.86))
                                .on_click(cx.listener(move |this, _, _, cx| this.open_book(details_asin.clone(), cx)))
                                .child(Cover::new(cover.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(220.), ui::metrics::radius::COVER_LARGE).elevation(Elevation::CoverHero)))
                            .child(div().flex_1().min_w_0().flex().flex_col()
                                .child(div().type_style(ui::text::CAPTION).text_color(color).child("Continue listening"))
                                .child(div().mt(px(8.)).type_style(ui::text::BOOK_DISPLAY).child(title))
                                .child(div().mt(px(8.)).type_style(ui::text::CALLOUT).text_color(color).child(credit))
                                .children(sentence.map(|sentence| div().mt(px(18.)).type_style(ui::text::BOOK_SENTENCE).child(sentence)))
                                .child(div().mt(px(20.)).flex().items_center().gap(px(10.))
                                    .child(self.resume_action("hero", &library.account_key, item, true, cx))
                                    .child(Button::new("hero-details").label("Details").on_click(cx.listener(move |this, _, _, cx| this.open_book(chapters_asin.clone(), cx)))))));
                        view = view.child(self.shelf("Still listening", "listening", &listening.iter().skip(1).take(6).copied().collect::<Vec<_>>(), 160., Page::Progress, cx));
                    }
                    view = view.child(self.shelf("Added recently", "recent", &recent, 118., Page::AllBooks, cx));
                    return view.into_any_element();
                }
                if self.page == Page::Progress {
                    let items: Vec<_> = library.items.iter().filter(|item| book_progress(item).unwrap_or(0.) > 0. && !book_finished(item)).collect();
                    if !items.is_empty() {
                        let remaining: u64 = items.iter().filter_map(|item| item.listening_status.as_ref().and_then(|status| status.time_remaining_seconds)).sum();
                        let key = library.account_key.clone();
                        return div().flex().flex_col()
                            .child(div().pb(px(12.)).border_b_1().border_color(hairline).type_style(ui::text::BODY).text_color(color)
                                .child(model::format::library_summary(items.len(), std::time::Duration::from_secs(remaining))))
                            .children(items.iter().enumerate().map(|(index, item)| self.progress_row(index, &key, item, cx)))
                            .into_any_element();
                    }
                }
                if self.page == Page::CatalogBook {
                    let Some(item) = &self.selected_catalog_book else { return base().child(heading("Book not found")).into_any_element() };
                    let url = Marketplace::ALL.iter().find(|market| market.display_name() == library.marketplace).map(|market| format!("https://www.audible.{}/pd/{}", market.domain(), item.asin));
                    let actions = div().flex().items_center().gap(px(10.))
                        .children(url.map(|url| Button::new("view-on-audible").label("View on Audible").icon(Icon::External).variant(ButtonVariant::Primary).on_click(move |_, _, cx| cx.open_url(&url))));
                    return self.book_detail("catalog-book-back", item, None, actions.into_any_element(), cx);
                }
                if self.page == Page::Book {
                    let Some(item) = self.selected_book.as_ref().and_then(|asin| library.items.iter().find(|item| &item.asin == asin)) else {
                        return base().child(heading("Book not found")).child(BackLink::new("book-back-missing", "Library").on_click(cx.listener(|this, _, _, cx| this.back(cx)))).into_any_element();
                    };
                    let local_path = backend::downloaded_path(&library.account_key, &item.asin);
                    let sentence = if book_finished(item) {
                        Some("You finished this book.".to_string())
                    } else {
                        item.listening_status.as_ref().and_then(|status| status.time_remaining_seconds)
                            .filter(|_| book_progress(item).unwrap_or(0.) > 0.)
                            .map(|seconds| format!("{}.", model::format::book_left_long(std::time::Duration::from_secs(seconds))))
                    };
                    let actions = div().flex().items_center().gap(px(10.))
                        .child(self.resume_action("book", &library.account_key, item, true, cx))
                        .children(local_path.map(|path| IconButton::new("book-reveal", Icon::Folder, 16.).hit_area(36.).hover_fill(theme_fill).tooltip("Show in Finder").on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(error) = std::process::Command::new("open").arg("-R").arg(&path).spawn() {
                                this.download_notice = Some(format!("Could not open Finder: {error}"));
                                cx.notify();
                            }
                        }))));
                    return self.book_detail("book-back", item, sentence, actions.into_any_element(), cx);
                }
                if self.page == Page::Explore {
                    let mut view = div().flex().flex_col().gap(px(36.));
                    if self.showing_search {
                        view = view.child(div().flex().child(BackLink::new("explore-home", "Explore").on_click(cx.listener(|this, _, _, cx| { this.showing_search = false; this.search_message = None; cx.notify(); }))))
                            .children(self.search_message.iter().map(|message| div().type_style(ui::text::BODY).text_color(color).child(message.clone())))
                            .children(self.search_results.iter().map(|item| {
                                let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                                let image = self.covers.get(&item.asin).cloned();
                                let product = item.clone();
                                div().id(format!("catalog-{}", item.asin)).cursor_pointer().rounded(px(8.)).hover(|style| style.bg(hover_fill)).active(|style| style.opacity(0.8)).flex().items_center().gap(px(18.)).pb(px(16.)).border_b_1().border_color(hairline)
                                    .on_click(cx.listener(move |this, _, _, cx| this.open_catalog_book(product.clone(), cx)))
                                    .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(72.), ui::metrics::radius::COVER).elevation(Elevation::CoverThumb))
                                    .child(div().flex().flex_col().gap(px(5.))
                                        .child(div().type_style(ui::text::BOOK_ROW).child(title))
                                        .child(div().type_style(ui::text::BODY).text_color(color).child(item.author_names())))
                            }));
                    } else if let Some(feed) = &self.explore_feed {
                        if let Some(message) = &feed.warning { view = view.child(div().type_style(ui::text::BODY).text_color(color).child(message.clone())); }
                        if let Some(item) = feed.new_releases.first() {
                            let product = item.clone();
                            let palette = self.palettes.get(&item.asin).copied().unwrap_or_default();
                            let image = self.covers.get(&item.asin).cloned();
                            view = view.child(div().rounded(px(ui::metrics::radius::FLOATING)).p(px(28.)).flex().items_center().gap(px(28.)).elevation(Elevation::Banner(palette), cx.theme(), window)
                                .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(168.), ui::metrics::radius::COVER_LARGE).elevation(Elevation::CoverOnField))
                                .child(div().flex_1().min_w_0().flex().flex_col()
                                    .child(div().type_style(ui::text::CAPTION).text_color(palette.on_field_muted).child("New release"))
                                    .child(div().mt(px(6.)).type_style(ui::text::BOOK_BANNER).text_color(palette.on_field).child(item.title.clone().unwrap_or_else(|| "Untitled".into())))
                                    .child(div().mt(px(6.)).type_style(ui::text::CALLOUT).text_color(palette.on_field_muted).child(item.author_names()))
                                    .child(div().mt(px(20.)).flex().child(Button::new("featured-book").label("View book").variant(ButtonVariant::Light(palette)).on_click(cx.listener(move |this, _, _, cx| this.open_catalog_book(product.clone(), cx)))))));
                        }
                        if !feed.genres.is_empty() {
                            view = view.child(div().flex().flex_col().gap(px(14.))
                                .child(SectionHeader::new("genres-header", "Browse by genre"))
                                .child(div().flex().gap(px(10.)).children(feed.genres.iter().take(6).enumerate().map(|(index, genre)| {
                                    let id = genre.id.clone(); let name = genre.name.clone(); let search_name = name.clone();
                                    let accent = ui::theme::Accent::resolve(ui::theme::AccentTheme::ALL[index % ui::theme::AccentTheme::ALL.len()], cx.theme().appearance, cx.theme().colors.window);
                                    div().id(("genre", index)).cursor_pointer().flex_1().min_w_0().h(px(54.)).px(px(14.)).flex().items_center().rounded(px(ui::metrics::radius::TILE))
                                        .bg(accent.tint).text_color(accent.on_tint).type_style(ui::text::BODY_MEDIUM)
                                        .hover(|style| style.opacity(0.84)).active(|style| style.opacity(0.72))
                                        .on_click(cx.listener(move |this, _, _, cx| this.browse_genre(id.clone(), search_name.clone(), cx)))
                                        .child(div().w_full().min_w_0().truncate().child(name))
                                }))));
                        }
                        if !feed.new_releases.is_empty() {
                            view = view.child(div().flex().flex_col().gap(px(14.))
                                .child(SectionHeader::new("releases-header", "New and noteworthy"))
                                .child(div().flex().flex_wrap().gap(px(20.)).children(feed.new_releases.iter().take(6).map(|item| {
                                    let product = item.clone(); let image = self.covers.get(&item.asin).cloned();
                                    div().id(format!("release-{}", item.asin)).w(px(160.)).flex_none().cursor_pointer()
                                        .hover(|style| style.opacity(0.86)).active(|style| style.opacity(0.72))
                                        .on_click(cx.listener(move |this, _, _, cx| this.open_catalog_book(product.clone(), cx)))
                                        .flex().flex_col()
                                        .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(160.), ui::metrics::radius::COVER).elevation(Elevation::CoverShelf))
                                        .child(div().mt(px(10.)).truncate().type_style(ui::text::CAPTION_MEDIUM).child(item.title.clone().unwrap_or_else(|| "Untitled".into())))
                                        .child(div().mt(px(2.)).truncate().type_style(ui::text::FOOTNOTE).text_color(color).child(item.author_names()))
                                }))));
                        }
                        if feed.warning.is_some() { view = view.child(Button::new("retry-explore").label("Try again").on_click(cx.listener(|this, _, _, cx| { this.explore_feed = None; this.load_explore(cx); cx.notify(); }))); }
                    } else {
                        if self.explore_loading {
                            view = view.child(loading_placeholders("Loading Explore…", cx));
                        } else {
                            view = view.child(div().type_style(ui::text::BODY).text_color(color).child("Explore is unavailable right now."))
                                .child(div().flex().child(Button::new("load-explore").label("Try again").on_click(cx.listener(|this, _, _, cx| this.load_explore(cx)))));
                        }
                    }
                    return view.into_any_element();
                }
                if matches!(self.page, Page::Authors | Page::Series) {
                    let authors = self.page == Page::Authors;
                    let mut groups = std::collections::BTreeMap::<String, usize>::new();
                    for item in &library.items {
                        if authors { for author in &item.authors { if !author.name.is_empty() { *groups.entry(author.name.clone()).or_default() += 1; } } }
                        else { for series in &item.series { if let Some(title) = &series.title && !title.is_empty() { *groups.entry(title.clone()).or_default() += 1; } } }
                    }
                    if groups.is_empty() { return base().child(heading(if authors { "No authors found" } else { "No series found" })).child(copy("These details are not available in your library yet.".into())).into_any_element(); }
                    return div().flex().flex_col().gap(px(6.)).children(groups.into_iter().enumerate().map(|(index, (name, count))| {
                        let selected = name.clone();
                        div().id(("browse-group", index)).cursor_pointer().px(px(12.)).py(px(14.)).rounded(px(8.))
                            .hover(|style| style.bg(hover_fill)).active(|style| style.opacity(0.8))
                            .flex().items_center().gap(px(20.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.browse_name = Some(selected.clone());
                                this.page = if authors { Page::AuthorBooks } else { Page::SeriesBooks };
                                this.navigation_generation += 1;
                                cx.notify();
                            }))
                            .child(div().flex_1().type_style(ui::text::BOOK_ROW).child(name))
                            .child(div().type_style(ui::text::CAPTION).text_color(color).child(format!("{count} {}", if count == 1 { "book" } else { "books" })))
                    })).into_any_element();
                }
                let query = self.library_search.read(cx).value().trim().to_lowercase();
                if self.page == Page::Search && query.is_empty() {
                    return base().child(copy("Search the books in your library.".into())).into_any_element();
                }
                let items: Vec<_> = library.items.iter().filter(|item| match self.page {
                    Page::Progress => book_progress(item).unwrap_or(0.) > 0. && !book_finished(item),
                    Page::OnThisMac => backend::downloaded_path(&library.account_key, &item.asin).is_some() || self.download.as_ref().is_some_and(|download| download.asin == item.asin),
                    Page::Finished => book_finished(item),
                    Page::Search => item.title.as_deref().unwrap_or("").to_lowercase().contains(&query) || item.author_names().to_lowercase().contains(&query) || item.narrator_names().to_lowercase().contains(&query),
                    Page::AuthorBooks => self.browse_name.as_ref().is_some_and(|name| item.authors.iter().any(|author| &author.name == name)),
                    Page::SeriesBooks => self.browse_name.as_ref().is_some_and(|name| item.series.iter().any(|series| series.title.as_ref() == Some(name))),
                    _ => true,
                }).collect();
                if items.is_empty() {
                    return base()
                        .child(heading(match self.page { Page::Progress => "Nothing in progress", Page::OnThisMac => "No downloads yet", Page::Search => "No matching books", Page::Finished => "No finished books", _ => "Your library is empty" }))
                        .child(copy(if self.page == Page::OnThisMac { "Download a title from your library to listen on this Mac.".into() } else if self.page == Page::Search { "Try a different title, author or narrator.".into() } else if library.offline { "Showing the library saved on this Mac.".into() } else { "Your Audible library has synced.".into() })).into_any_element();
                }
                div().flex().flex_col().gap(px(16.))
                    .children(matches!(self.page, Page::AuthorBooks | Page::SeriesBooks).then(|| {
                        let target = if self.page == Page::AuthorBooks { Page::Authors } else { Page::Series };
                        div().flex().items_center().gap(px(16.))
                            .child(BackLink::new("browse-back", page_name(target)).on_click(cx.listener(move |this, _, _, cx| { this.page = target; cx.notify(); })))
                            .child(div().type_style(ui::text::BOOK_TITLE).child(self.browse_name.clone().unwrap_or_default()))
                    }))
                    .child(div().type_style(ui::text::BODY).text_color(color).child(format!("{} titles", items.len())))
                    .children(items.into_iter().map(|item| {
                        let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                        let author = item.author_names();
                        let image = self.covers.get(&item.asin).cloned();
                        let asin = item.asin.clone();
                        let key = library.account_key.clone();
                        let button_id = format!("download-{}", asin);
                        let local_path = backend::downloaded_path(&key, &asin);
                        let downloaded = local_path.is_some();
                        let active = self.download.as_ref().filter(|download| download.asin == asin);
                        let active_stage = active.map(|download| download.event.lock().map(|event| download_display(*event).0).unwrap_or("Preparing download"));
                        let other_download = self.download.is_some() && active.is_none();
                        let download_title = title.clone();
                        let open_asin = asin.clone();
                        let open_id = format!("book-{asin}");
                        let progress = book_progress(item).map(|v| format!("{v:.0}% complete")).unwrap_or_else(|| "Not started".into());
                        div().flex().items_center().gap(px(20.)).pb(px(16.)).border_b_1().border_color(hairline)
                            .child(div().id(open_id).cursor_pointer().rounded(px(8.)).hover(|style| style.bg(hover_fill)).active(|style| style.opacity(0.8)).flex_1().min_w_0().flex().items_center().gap(px(20.))
                                .on_click(cx.listener(move |this, _, _, cx| this.open_book(open_asin.clone(), cx)))
                                .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(76.), ui::metrics::radius::COVER).elevation(Elevation::CoverThumb))
                                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(5.))
                                    .child(div().type_style(ui::text::BOOK_ROW).child(title))
                                    .child(div().type_style(ui::text::BODY).text_color(color).child(author))))
                            .child(div().type_style(ui::text::BODY).text_color(color).child(progress))
                            .child(if let Some(stage) = active_stage {
                                div().type_style(ui::text::BODY).text_color(color).child(format!("{stage}…")).into_any_element()
                            } else if downloaded {
                                div().flex().items_center().gap(px(8.))
                                    .child(IconButton::new(format!("reveal-{asin}"), Icon::Folder, 16.).hit_area(30.).hover_fill(theme_fill).tooltip("Show in Finder").on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(path) = &local_path && let Err(error) = std::process::Command::new("open").arg("-R").arg(path).spawn() {
                                            this.download_notice = Some(format!("Could not open Finder: {error}")); cx.notify();
                                        }
                                    })))
                                    .child(IconButton::new(button_id, Icon::Play, 16.).hit_area(30.).color(theme_ink).hover_fill(theme_fill).tooltip("Play").on_click(cx.listener(move |this, _, _, cx| this.start_player(key.clone(), asin.clone(), download_title.clone(), cx)))).into_any_element()
                            } else if other_download {
                                div().type_style(ui::text::CAPTION).text_color(color).child("One download at a time").into_any_element()
                            } else {
                                IconButton::new(button_id, Icon::Download, 16.).hit_area(30.).color(theme_ink).hover_fill(theme_fill).tooltip("Download to this Mac").on_click(cx.listener(move |this, _, _, cx| this.download(key.clone(), asin.clone(), download_title.clone(), cx))).into_any_element()
                            })
                    })).into_any_element()
            }
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.page == Page::NowPlaying && self.player.is_some() {
            return self.player_view(window, cx);
        }
        let theme = cx.theme().clone();
        let title = match self.page {
            Page::Library => "Library",
            Page::AllBooks => "All books",
            Page::Progress => "In progress",
            Page::Explore => "Explore",
            Page::OnThisMac => "On this Mac",
            Page::Bookmarks => "Bookmarks",
            Page::Search => "Search",
            Page::Finished => "Finished",
            Page::Authors | Page::AuthorBooks => "Authors",
            Page::Series | Page::SeriesBooks => "Series",
            Page::Book | Page::CatalogBook => "Book",
            Page::NowPlaying => "Now playing",
            Page::Settings => "Appearance",
        };
        let account_label = match &self.account {
            Account::Ready(library) => format!(
                "Audible {}{}",
                library.marketplace,
                if library.offline { " · Offline" } else { "" }
            ),
            _ => "Not connected".into(),
        };
        let notice = if matches!(self.account, Account::Ready(_)) {
            self.download_notice.as_ref().map(|message| {
                div()
                    .mx(px(36.))
                    .mt(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .bg(theme.colors.fill)
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .type_style(ui::text::BODY)
                            .text_color(theme.colors.ink)
                            .child(message.clone()),
                    )
                    .child(
                        IconButton::new("dismiss-status", Icon::Cancel, 14.)
                            .hit_area(24.)
                            .hover_fill(theme.colors.fill)
                            .tooltip("Dismiss")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.download_notice = None;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
        } else {
            None
        };
        let download_panel = self.download.as_ref().map(|download| {
            let event = download
                .event
                .lock()
                .map(|event| *event)
                .unwrap_or(audible::DownloadEvent::Licensing);
            let (stage, detail, fraction) = download_display(event);
            let title = download.title.clone();
            let mut track = div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(theme.colors.hairline);
            if let Some(fraction) = fraction {
                track = track.child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded_full()
                        .bg(theme.accent.fill),
                );
            }
            div()
                .mx(px(36.))
                .mt(px(12.))
                .px(px(16.))
                .py(px(12.))
                .rounded(px(10.))
                .bg(theme.accent.tint)
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .type_style(ui::text::BODY)
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(theme.accent.on_tint)
                                        .child(format!("{stage} · {title}")),
                                )
                                .child(
                                    div()
                                        .type_style(ui::text::CAPTION)
                                        .text_color(theme.colors.ink_secondary)
                                        .child(detail),
                                ),
                        )
                        .child(
                            IconButton::new("view-downloads", Icon::ChevronRight, 16.)
                                .hit_area(28.)
                                .hover_fill(theme.colors.fill)
                                .tooltip("Show downloads")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.page = Page::OnThisMac;
                                    cx.notify();
                                })),
                        ),
                )
                .child(track)
                .into_any_element()
        });
        let bar = self.player.as_ref().map(|active| {
            let compact_bar = f32::from(window.viewport_size().width) < 1050.;
            let title = active.title.clone();
            let image = self.covers.get(&active.asin).cloned();
            let playing = active.player.is_playing();
            let position = active.player.position().max(0.);
            let duration = active.player.duration().max(0.);
            let fraction = if duration > 0. {
                (position / duration).clamp(0., 1.) as f32
            } else {
                0.
            };
            let elapsed = model::format::clock(std::time::Duration::from_secs(position as u64));
            let remaining = model::format::countdown(std::time::Duration::from_secs(
                (duration - position).max(0.) as u64,
            ));
            let speed = active.player.rate();
            let subtitle = current_chapter(&active.chapters, (position * 1000.) as u64)
                .map(|(index, chapter)| {
                    if chapter.title.is_empty() {
                        format!("Chapter {}", index + 1)
                    } else {
                        chapter.title.clone()
                    }
                })
                .unwrap_or_else(|| "Now playing".into());
            let sleeping = !matches!(self.sleep, SleepTimer::Off);
            let sleep_tip = if sleeping {
                self.sleep.label(std::time::Instant::now())
            } else {
                "Sleep timer".into()
            };
            let weak = cx.entity().downgrade();
            let scrubber = Scrubber::new("bar-seek", fraction, ScrubberColors::window(cx)).on_seek(
                move |value, _, cx| {
                    let _ = weak.update(cx, |this, cx| this.seek_fraction(value, cx));
                },
            );
            div()
                .mx(px(24.))
                .mb(px(20.))
                .h(px(76.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(if compact_bar { 8. } else { 16. }))
                .px(px(14.))
                .rounded(px(18.))
                .elevation(Elevation::FloatingPlayer, &theme, window)
                .child(
                    div()
                        .id("open-now-playing")
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.open_player(cx)))
                        .w(px(if compact_bar { 165. } else { 260. }))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            Cover::new(
                                image
                                    .map(gpui_kit::ImageSource::from)
                                    .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                CoverSize::Square(48.),
                                ui::metrics::radius::COVER,
                            )
                            .elevation(Elevation::CoverThumb),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .truncate()
                                        .type_style(ui::text::BODY_MEDIUM)
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .type_style(ui::text::CAPTION)
                                        .text_color(theme.colors.ink_secondary)
                                        .child(subtitle),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(if compact_bar { 12. } else { 24. }))
                                .child(
                                    IconButton::new("back-30", Icon::Back30, 22.)
                                        .hit_area(30.)
                                        .color(theme.colors.ink)
                                        .tooltip("Back 30 seconds")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.skip(-30., cx)),
                                        ),
                                )
                                .child(
                                    Button::new("toggle-play")
                                        .icon(if playing { Icon::Pause } else { Icon::Play })
                                        .variant(ButtonVariant::Ink)
                                        .shape(ButtonShape::Circle {
                                            diameter: 36.,
                                            icon: 16.,
                                        })
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.toggle_playback(cx)),
                                        ),
                                )
                                .child(
                                    IconButton::new("forward-30", Icon::Forward30, 22.)
                                        .hit_area(30.)
                                        .color(theme.colors.ink)
                                        .tooltip("Forward 30 seconds")
                                        .on_click(cx.listener(|this, _, _, cx| this.skip(30., cx))),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .max_w(px(500.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .type_style(ui::text::FOOTNOTE.tabular())
                                        .text_color(theme.colors.ink_secondary)
                                        .child(elapsed),
                                )
                                .child(div().flex_1().child(scrubber))
                                .child(
                                    div()
                                        .type_style(ui::text::FOOTNOTE.tabular())
                                        .text_color(theme.colors.ink_secondary)
                                        .child(remaining),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            Chip::new("bar-speed")
                                .label(model::format::speed(speed))
                                .on_click(cx.listener(|this, _, _, cx| this.cycle_speed(cx))),
                        )
                        .child(
                            IconButton::new("bar-sleep", Icon::Moon, 16.)
                                .hit_area(28.)
                                .color(if sleeping {
                                    theme.accent.text
                                } else {
                                    theme.colors.ink_secondary
                                })
                                .hover_fill(theme.colors.fill)
                                .tooltip(sleep_tip)
                                .on_click(cx.listener(|this, _, _, cx| this.cycle_sleep(cx))),
                        )
                        .child(
                            IconButton::new("bar-chapters", Icon::Chapters, 16.)
                                .hit_area(28.)
                                .hover_fill(theme.colors.fill)
                                .tooltip("Chapters")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.chapters_open = true;
                                    this.open_player(cx);
                                })),
                        )
                        .children((!compact_bar).then(|| {
                            IconButton::new("bar-mini", Icon::Pip, 16.)
                                .hit_area(28.)
                                .hover_fill(theme.colors.fill)
                                .tooltip("Open mini player")
                                .on_click(cx.listener(|this, _, _, cx| this.open_mini_player(cx)))
                        })),
                )
                .into_any_element()
        });
        let content = div().w_full().child(self.account_view(window, cx));
        let content = if matches!(self.page, Page::Book | Page::CatalogBook) && !cx.reduce_motion()
        {
            gpui_kit::base::animation::EffectTransition::new(std::time::Duration::from_millis(180))
                .slide_y(px(8.), px(0.))
                .fade(0.90, 1.0)
                .apply(
                    content.relative(),
                    format!("book-enter-{}", self.navigation_generation),
                )
                .into_any_element()
        } else {
            content.into_any_element()
        };
        div()
            .size_full()
            .flex()
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                let input = if matches!(this.page, Page::Explore | Page::CatalogBook) {
                    &this.search
                } else {
                    &this.library_search
                };
                window.focus(&gpui_kit::Focusable::focus_handle(input, cx), cx);
            }))
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
                    .child(self.nav("On this Mac", Icon::Download, Page::OnThisMac, cx))
                    .child(self.nav("Bookmarks", Icon::Bookmark, Page::Bookmarks, cx))
                    .child(div().h(px(20.)))
                    .child(SidebarHeader::new("Browse"))
                    .child(self.nav("Authors", Icon::User, Page::Authors, cx))
                    .child(self.nav("Series", Icon::Layers, Page::Series, cx))
                    .child(self.nav("Finished", Icon::CheckmarkCircle, Page::Finished, cx))
                    .child(div().flex_1())
                    .child(self.nav("Appearance", Icon::Settings, Page::Settings, cx))
                    .child(
                        div()
                            .px(px(10.))
                            .pb(px(25.))
                            .type_style(ui::text::CAPTION)
                            .text_color(theme.colors.ink_secondary)
                            .child(account_label),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(Toolbar::new(title).trailing(
                        if matches!(self.page, Page::Explore | Page::CatalogBook) {
                            SearchField::new(&self.search, "⌘F")
                        } else {
                            SearchField::new(&self.library_search, "⌘F")
                        },
                    ))
                    .children(download_panel)
                    .children(notice)
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .overflow_y_scroll()
                            .px(px(36.))
                            .pt(px(32.))
                            .pb(px(36.))
                            .child(content),
                    )
                    .children(bar),
            )
            .into_any_element()
    }
}

#[cfg(debug_assertions)]
fn smoke_open_mini(handle: gpui_kit::WindowHandle<Root>, cx: &mut gpui_kit::App) {
    let Some(path) = std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO") else {
        return;
    };
    let result = handle.update(cx, |root, _, cx| {
        let shell = root
            .view()
            .clone()
            .downcast::<Shell>()
            .map_err(|_| anyhow::anyhow!("Smoke test could not find the app view"))?;
        shell.update(cx, |shell, cx| {
            let player = player::Player::open(std::path::Path::new(&path))?;
            shell.player = Some(ActivePlayer {
                asin: audible::Asin::new("SMOKETEST")?,
                account_key: "0".repeat(64),
                title: "Test audio".into(),
                player,
                was_playing: false,
                chapters: Vec::new(),
                chapters_loading: false,
            });
            shell.open_player(cx);
            shell.open_mini_player(cx);
            Ok::<(), anyhow::Error>(())
        })
    });
    match result {
        Ok(Ok(())) => eprintln!("Smoke test: mini player requested"),
        Ok(Err(error)) => {
            eprintln!("Smoke test failed: {error:#}");
            cx.quit();
        }
        Err(error) => {
            eprintln!("Smoke test failed: {error:#}");
            cx.quit();
        }
    }
}

gpui_kit::actions!(
    better_audible,
    [
        /// Focuses the toolbar search field (⌘F).
        FocusSearch
    ]
);

fn main() {
    gpui_kit::application().with_assets(ui::Assets).run(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([gpui_kit::KeyBinding::new("cmd-f", FocusSearch, None)]);
        if let Err(error) = ui::init(backend::load_appearance(), cx) {
            eprintln!("Could not initialize the interface: {error:#}");
            cx.quit();
            return;
        }
        let bounds = Bounds::new(point(px(100.), px(100.)), size(px(1200.), px(800.)));
        match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                show: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                focus: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                titlebar: Some(TitlebarOptions {
                    appears_transparent: cfg!(target_os = "macos"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Shell::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            Ok(_handle) => {
                #[cfg(debug_assertions)]
                smoke_open_mini(_handle, cx);
                if std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none() {
                    cx.activate(true);
                }
            }
            Err(error) => {
                eprintln!("Could not open the app window: {error:#}");
                cx.quit();
            }
        }
    });
}

#[cfg(test)]
mod download_tests {
    use super::*;
    use audible::{DownloadEvent, download::Progress};

    #[test]
    fn sleep_timer_only_stops_at_its_deadline_or_matching_chapter_end() {
        let now = std::time::Instant::now();
        let timed = SleepTimer::Deadline(now + std::time::Duration::from_secs(30));
        assert!(!timed.due(now, None));
        assert!(timed.due(now + std::time::Duration::from_secs(30), None));
        let book = audible::Asin::new("BOOK1").unwrap();
        let other = audible::Asin::new("BOOK2").unwrap();
        let chapter = SleepTimer::EndOfChapter {
            asin: book.clone(),
            end_seconds: 120.,
        };
        assert!(!chapter.due(now, Some((&book, 119.9))));
        assert!(chapter.due(now, Some((&book, 120.))));
        assert!(!chapter.due(now, Some((&other, 120.))));
        assert!(!SleepTimer::Off.due(now, None));
        assert_eq!(
            strip_html("<p>A <b>tale</b> of&nbsp;two &amp; more.</p>"),
            "A tale of two & more."
        );
    }

    #[test]
    fn chapter_lookup_respects_boundaries_and_missing_metadata() {
        let chapters = vec![
            audible::models::Chapter {
                title: "One".into(),
                start_offset_ms: 0,
                length_ms: 1_000,
                chapters: vec![],
            },
            audible::models::Chapter {
                title: "Two".into(),
                start_offset_ms: 1_000,
                length_ms: 2_000,
                chapters: vec![],
            },
        ];
        assert_eq!(
            current_chapter(&chapters, 999).map(|(index, _)| index),
            Some(0)
        );
        assert_eq!(
            current_chapter(&chapters, 1_000).map(|(index, _)| index),
            Some(1)
        );
        assert!(current_chapter(&chapters, 3_000).is_none());
        assert!(current_chapter(&[], 0).is_none());
    }

    #[test]
    fn progress_is_stage_specific_and_unknown_totals_stay_indeterminate() {
        assert_eq!(download_display(DownloadEvent::Licensing).2, None);
        let (_, detail, fraction) = download_display(DownloadEvent::Downloading(Progress {
            downloaded: 1_000_000,
            total: Some(4_000_000),
        }));
        assert_eq!(fraction, Some(0.25));
        assert_eq!(detail, "25% · 1.0 of 4.0 MB");
        let (_, detail, fraction) = download_display(DownloadEvent::Downloading(Progress {
            downloaded: 2_000_000,
            total: None,
        }));
        assert_eq!(fraction, None);
        assert_eq!(detail, "2.0 MB downloaded");
        let (stage, _, fraction) = download_display(DownloadEvent::Decrypting(Progress {
            downloaded: 50,
            total: Some(100),
        }));
        assert_eq!(stage, "Preparing audio");
        assert_eq!(fraction, Some(0.5));
    }
}
