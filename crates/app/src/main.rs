//! Better Audible's native application shell.
mod backend;
mod player;
mod settings;

use audible::{Marketplace, auth::LoginSession};
use gpui_kit::{
    AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
    component::{
        Root,
        input::{Input, InputEvent, InputState},
        select::{SearchableVec, Select, SelectEvent, SelectState},
    },
    div, linear_color_stop, linear_gradient, point, px, relative, size,
};
use ui::{
    ActiveTheme, Elevation, ElevationExt, Icon,
    components::{
        Button, ButtonShape, ButtonVariant, Cover, CoverSize, Scrubber, ScrubberColors,
        SidebarHeader, SidebarItem, Toolbar,
    },
};

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
    previous_page: Page,
    selected_book: Option<audible::Asin>,
    selected_catalog_book: Option<audible::models::Product>,
    seek_generation: u64,
    navigation_generation: u64,
    chapters_open: bool,
    bookmarks: Vec<(audible::Asin, u64)>,
    bookmarks_loading: bool,
    sleep: SleepTimer,
    sleep_select: Entity<SelectState<SearchableVec<&'static str>>>,
    account: Account,
    marketplace: Entity<SelectState<SearchableVec<&'static str>>>,
    redirect: Entity<InputState>,
    search: Entity<InputState>,
    library_search: Entity<InputState>,
    browse_name: Option<String>,
    _subscriptions: Vec<gpui_kit::Subscription>,
    search_results: Vec<audible::models::Product>,
    searching: bool,
    search_message: Option<String>,
    download_notice: Option<String>,
    download: Option<ActiveDownload>,
    player: Option<ActivePlayer>,
    loading_player: bool,
    covers: std::collections::HashMap<audible::Asin, std::path::PathBuf>,
    palettes: std::collections::HashMap<audible::Asin, ui::theme::BookPalette>,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let redirect =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste the final URL here"));
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search titles, authors or narrators")
        });
        let library_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search titles, authors or narrators")
        });
        let sleep_options = SearchableVec::new(vec![
            "Off",
            "15 minutes",
            "30 minutes",
            "1 hour",
            "End of chapter",
        ]);
        let sleep_select = cx.new(|cx| SelectState::new(sleep_options, None, window, cx));
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, window, cx| {
                ui::Theme::sync_system_appearance(window.appearance(), cx)
            }),
            cx.subscribe(
                &library_search,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ),
            cx.subscribe(&search, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.search_catalog(cx);
                }
            }),
            cx.subscribe_in(
                &sleep_select,
                window,
                |this: &mut Self,
                 select,
                 event: &SelectEvent<SearchableVec<&'static str>>,
                 window,
                 cx| {
                    if let SelectEvent::Confirm(Some(choice)) = event {
                        this.set_sleep(choice, cx);
                        select.update(cx, |select, cx| select.set_selected_index(None, window, cx));
                    }
                },
            ),
        ];
        let marketplaces = SearchableVec::new(
            Marketplace::ALL
                .iter()
                .map(|m| m.display_name())
                .collect::<Vec<_>>(),
        );
        let marketplace = cx.new(|cx| SelectState::new(marketplaces, None, window, cx));
        let mut shell = Self {
            page: Page::Library,
            previous_page: Page::Library,
            selected_book: None,
            selected_catalog_book: None,
            seek_generation: 0,
            navigation_generation: 0,
            chapters_open: false,
            bookmarks: Vec::new(),
            bookmarks_loading: false,
            sleep: SleepTimer::Off,
            sleep_select,
            account: Account::Loading,
            marketplace,
            redirect,
            search,
            library_search,
            browse_name: None,
            _subscriptions: subscriptions,
            search_results: Vec::new(),
            searching: false,
            search_message: None,
            download_notice: None,
            download: None,
            player: None,
            loading_player: false,
            covers: Default::default(),
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
        self.search_message = Some("Searching Audible…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::search_catalog(query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.searching = false;
                match result {
                    Ok(products) => {
                        this.search_message = Some(if products.is_empty() {
                            "No results found.".into()
                        } else {
                            format!("{} results", products.len())
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
        covers.extend(self.search_results.iter().filter_map(|item| {
            item.cover_url(500)
                .map(|url| (item.asin.clone(), url.to_string()))
        }));
        covers.retain(|(asin, _)| !self.covers.contains_key(asin));
        cx.spawn(async move |this, cx| {
            for (asin, url) in covers {
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
                if let Ok((path, palette)) = result
                    && this
                        .update(cx, |this, cx| {
                            if let Some(palette) = palette {
                                this.palettes.insert(key.clone(), palette);
                            }
                            this.covers.insert(key, path);
                            cx.notify();
                        })
                        .is_err()
                {
                    break;
                }
            }
        })
        .detach();
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

    fn player_view(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(active) = &self.player else {
            return div().into_any_element();
        };
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
        let total = model::format::clock(std::time::Duration::from_secs(segment_length as u64));
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
                .w(px(260.))
                .h_full()
                .flex_none()
                .overflow_y_scroll()
                .p(px(16.))
                .rounded(px(10.))
                .bg(palette.on_field_track)
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("Chapters"),
                );
            if active.chapters.is_empty() {
                panel = panel.child(
                    div()
                        .mt(px(12.))
                        .text_size(px(13.))
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
                        .text_size(px(13.))
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
                .fade(0.90, 1.0)
                .apply(panel, "chapter-panel-enter")
                .into_any_element()
            };
            Some(panel)
        } else {
            None
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
                    .h(px(58.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(100.))
                    .pr(px(28.))
                    .child(
                        Button::new("player-back")
                            .label("Back")
                            .icon(Icon::ArrowLeft)
                            .variant(ButtonVariant::Glass(palette))
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(12.))
                            .text_size(px(12.))
                            .text_color(palette.on_field_muted)
                            .children(self.download_notice.iter().cloned()),
                    )
                    .child(
                        Button::new("add-bookmark")
                            .label("Bookmark")
                            .icon(Icon::BookmarkAdd)
                            .variant(ButtonVariant::Glass(palette))
                            .on_click(cx.listener(|this, _, _, cx| this.add_current_bookmark(cx))),
                    )
                    .child(
                        Button::new("show-chapters")
                            .label(if self.chapters_open {
                                "Hide chapters"
                            } else {
                                "Chapters"
                            })
                            .icon(Icon::Chapters)
                            .variant(ButtonVariant::Glass(palette))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.chapters_open = !this.chapters_open;
                                cx.notify();
                            })),
                    )
                    .children(downloading.map(|message| {
                        div()
                            .text_size(px(12.))
                            .text_color(palette.on_field_muted)
                            .child(message)
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(64.))
                    .px(px(56.))
                    .child(
                        Cover::new(
                            image
                                .map(gpui_kit::ImageSource::from)
                                .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                            CoverSize::Square(320.),
                            8.,
                        )
                        .elevation(Elevation::CoverStage),
                    )
                    .child(
                        div()
                            .flex_1()
                            .max_w(px(560.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_family("Literata")
                                    .text_size(px(24.))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(6.))
                                    .text_size(px(14.))
                                    .text_color(palette.on_field_muted)
                                    .child(credit),
                            )
                            .child(
                                div()
                                    .mt(px(40.))
                                    .font_family("Geist Pixel")
                                    .text_size(px(104.))
                                    .child(left),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .font_family("Literata")
                                    .text_size(px(19.))
                                    .child(caption),
                            )
                            .child(div().mt(px(32.)).child(scrubber))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .mt(px(6.))
                                    .text_size(px(12.))
                                    .text_color(palette.on_field_muted)
                                    .child(time)
                                    .child(total),
                            ),
                    )
                    .children(chapter_panel),
            )
            .child(
                div()
                    .h(px(120.))
                    .flex_none()
                    .px(px(56.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Button::new("full-speed")
                                    .label(format!("{speed}×"))
                                    .variant(ButtonVariant::Glass(palette))
                                    .on_click(cx.listener(|this, _, _, cx| this.cycle_speed(cx))),
                            )
                            .child(
                                div().w(px(165.)).child(
                                    Select::new(&self.sleep_select)
                                        .placeholder("Sleep timer")
                                        .accessibility_label("Sleep timer")
                                        .menu_width(px(180.)),
                                ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(palette.on_field_muted)
                                    .child(self.sleep.label(std::time::Instant::now())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(30.))
                            .child(
                                Button::new("full-back30")
                                    .icon(Icon::Back30)
                                    .variant(ButtonVariant::Glass(palette))
                                    .shape(ButtonShape::Circle {
                                        diameter: 44.,
                                        icon: 24.,
                                    })
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
                                Button::new("full-forward30")
                                    .icon(Icon::Forward30)
                                    .variant(ButtonVariant::Glass(palette))
                                    .shape(ButtonShape::Circle {
                                        diameter: 44.,
                                        icon: 24.,
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.skip(30., cx))),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(palette.on_field_muted)
                                    .child("Volume"),
                            )
                            .child(div().w(px(100.)).child(volume_control)),
                    ),
            );
        // Opacity-only transition keeps spatial motion out of reduced-motion mode.
        if cx.reduce_motion() {
            return stage.into_any_element();
        }
        gpui_kit::base::animation::EffectTransition::new(std::time::Duration::from_millis(160))
            .fade(0.92, 1.0)
            .apply(
                stage,
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
            .selected(self.page == page)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                if page == Page::Bookmarks {
                    this.load_bookmarks(cx);
                }
                cx.notify();
            }))
    }

    fn shelf(
        &self,
        title: &'static str,
        id: &'static str,
        books: &[&audible::models::LibraryItem],
        target: Page,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(19.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(Button::new(format!("{id}-all")).label("See all").on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.page = target;
                            this.navigation_generation += 1;
                            cx.notify();
                        }),
                    )),
            )
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
                            .map(|seconds| {
                                model::format::time_left(std::time::Duration::from_secs(seconds))
                            })
                            .or_else(|| {
                                book_progress(item).map(|percent| format!("{percent:.0}% complete"))
                            })
                            .unwrap_or(author);
                        div()
                            .id(format!("{id}-{asin}"))
                            .w(px(142.))
                            .flex_none()
                            .cursor_pointer()
                            .rounded(px(8.))
                            .hover(|style| style.opacity(0.86))
                            .active(|style| style.opacity(0.72))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_book(open_asin.clone(), cx)
                            }))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(Cover::new(
                                cover
                                    .map(gpui_kit::ImageSource::from)
                                    .unwrap_or_else(|| gpui_kit::ImageSource::from("")),
                                CoverSize::Square(142.),
                                7.,
                            ))
                            .child(
                                div()
                                    .mt(px(4.))
                                    .font_family("Literata")
                                    .text_size(px(15.))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.colors.ink_secondary)
                                    .child(detail),
                            )
                    })),
            )
            .into_any_element()
    }

    fn account_view(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if self.page == Page::Settings {
            return self.settings_view(cx);
        }
        let color = cx.theme().colors.ink_secondary;
        let hairline = cx.theme().colors.hairline;
        let hover_fill = cx.theme().colors.fill;
        let heading = |title: &'static str| {
            div()
                .font_family("Literata")
                .text_size(px(32.))
                .child(title)
        };
        let copy = |text: String| {
            div()
                .max_w(px(570.))
                .text_size(px(14.))
                .text_color(color)
                .child(text)
        };
        let base = || div().flex().flex_col().gap(px(18.)).max_w(px(620.));
        match &self.account {
            Account::Loading => base().child(heading("Connecting to Audible"))
                .child(copy("Loading your account and library…".into())).into_any_element(),
            Account::SignedOut => base().child(heading("Your Audible library, here"))
                .child(copy("Choose your Audible marketplace, then sign in with Amazon. Your device credentials are stored in your Mac’s Keychain.".into()))
                .child(div().flex().gap(px(12.))
                    .child(div().w(px(220.)).child(Select::new(&self.marketplace).placeholder("Choose marketplace").accessibility_label("Audible marketplace")))
                    .child(Button::new("sign-in").label("Sign in with Amazon").variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.begin_sign_in(cx)))))
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
                    if self.bookmarks_loading { return base().child(copy("Loading bookmarks…".into())).into_any_element(); }
                    if self.bookmarks.is_empty() { return base().child(heading("No bookmarks yet")).child(copy("Save a moment from the player and find it here. Bookmarks are stored on this Mac.".into())).into_any_element(); }
                    return div().flex().flex_col().gap(px(12.))
                        .child(div().text_size(px(13.)).text_color(color).child("Saved on this Mac"))
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
                                    .child(div().font_family("Literata").text_size(px(18.)).child(title))
                                    .child(div().text_size(px(12.)).text_color(color).child(time)))
                                .child(if downloaded {
                                    Button::new(format!("bookmark-play-{asin}-{position}")).label("Play from here").icon(Icon::Play).on_click(cx.listener(move |this, _, _, cx| this.start_player_at(key.clone(), play_asin.clone(), play_title.clone(), Some(bookmark_position), cx)))
                                } else {
                                    Button::new(format!("bookmark-book-{asin}-{position}")).label("Open book").on_click(cx.listener(move |this, _, _, cx| this.open_book(play_asin.clone(), cx)))
                                })
                                .child(Button::new(format!("bookmark-remove-{asin}-{position}")).label("Remove").on_click(cx.listener(move |this, _, _, cx| this.change_bookmark(remove_asin.clone(), bookmark_position, false, cx)))))
                        })).into_any_element();
                }
                if self.page == Page::Library && !library.items.is_empty() {
                    let listening: Vec<_> = library.items.iter().filter(|item| book_progress(item).unwrap_or(0.) > 0. && !book_finished(item)).collect();
                    let recent: Vec<_> = library.items.iter().take(8).collect();
                    let mut view = div().flex().flex_col().gap(px(32.));
                    if let Some(item) = listening.first() {
                        let asin = item.asin.clone();
                        let details_asin = asin.clone();
                        let key = library.account_key.clone();
                        let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                        let author = item.author_names();
                        let cover = self.covers.get(&asin).cloned();
                        let downloaded = backend::downloaded_path(&key, &asin).is_some();
                        let active_download = self.download.as_ref().is_some_and(|download| download.asin == asin);
                        let current = self.player.as_ref().is_some_and(|player| player.asin == asin);
                        let action = if active_download { div().text_size(px(13.)).text_color(color).child("Download in progress").into_any_element() }
                        else if current { Button::new("hero-player").label("Open player").icon(Icon::Play).variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.open_player(cx))).into_any_element() }
                        else if downloaded { let book_title = title.clone(); Button::new("hero-resume").label("Resume").icon(Icon::Play).variant(ButtonVariant::Primary).on_click(cx.listener(move |this, _, _, cx| this.start_player(key.clone(), asin.clone(), book_title.clone(), cx))).into_any_element() }
                        else if self.download.is_some() { div().text_size(px(13.)).text_color(color).child("Another download is in progress").into_any_element() }
                        else { let book_title = title.clone(); Button::new("hero-download").label("Download").icon(Icon::Download).variant(ButtonVariant::Primary).on_click(cx.listener(move |this, _, _, cx| this.download(key.clone(), asin.clone(), book_title.clone(), cx))).into_any_element() };
                        view = view.child(div().flex().items_center().gap(px(32.))
                            .child(div().id("hero-book").cursor_pointer().hover(|style| style.opacity(0.86)).active(|style| style.opacity(0.72))
                                .on_click(cx.listener(move |this, _, _, cx| this.open_book(details_asin.clone(), cx)))
                                .child(Cover::new(cover.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(200.), 7.).elevation(Elevation::CoverHero)))
                            .child(div().flex().flex_col().gap(px(12.))
                                .child(div().text_size(px(12.)).text_color(color).child("Continue listening"))
                                .child(div().font_family("Literata").text_size(px(36.)).child(title))
                                .child(div().text_size(px(14.)).text_color(color).child(author))
                                .children(book_progress(item).map(|percent| div().font_family("Literata").text_size(px(17.)).child(format!("{percent:.0}% complete"))))
                                .child(action)));
                        view = view.child(self.shelf("Still listening", "listening", &listening.iter().take(6).copied().collect::<Vec<_>>(), Page::Progress, cx));
                    }
                    view = view.child(self.shelf("Recently added", "recent", &recent, Page::AllBooks, cx));
                    return view.into_any_element();
                }
                if self.page == Page::CatalogBook {
                    let Some(item) = &self.selected_catalog_book else { return base().child(heading("Book not found")).into_any_element() };
                    let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                    let author = item.author_names();
                    let narrator = item.narrator_names();
                    let cover = self.covers.get(&item.asin).cloned();
                    let url = Marketplace::ALL.iter().find(|market| market.display_name() == library.marketplace).map(|market| format!("https://www.audible.{}/pd/{}", market.domain(), item.asin));
                    return div().flex().flex_col().gap(px(28.))
                        .child(Button::new("catalog-book-back").label("Back to Explore").icon(Icon::ArrowLeft).on_click(cx.listener(|this, _, _, cx| this.back(cx))))
                        .child(div().flex().items_start().gap(px(32.))
                            .child(Cover::new(cover.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(220.), 7.))
                            .child(div().flex().flex_col().gap(px(12.)).max_w(px(600.))
                                .child(div().font_family("Literata").text_size(px(34.)).child(title))
                                .child(div().text_size(px(14.)).text_color(color).child(author))
                                .children((!narrator.is_empty()).then(|| div().text_size(px(13.)).text_color(color).child(format!("Read by {narrator}"))))
                                .children(item.runtime_length_min.map(|mins| div().text_size(px(13.)).text_color(color).child(format!("{} hr {} min", mins / 60, mins % 60))))
                                .children(url.map(|url| Button::new("view-on-audible").label("View on Audible").variant(ButtonVariant::Primary).on_click(move |_, _, cx| cx.open_url(&url))))))
                        .into_any_element();
                }
                if self.page == Page::Book {
                    let Some(item) = self.selected_book.as_ref().and_then(|asin| library.items.iter().find(|item| &item.asin == asin)) else {
                        return base().child(heading("Book not found")).child(Button::new("book-back-missing").label("Back to library").on_click(cx.listener(|this, _, _, cx| this.back(cx)))).into_any_element();
                    };
                    let asin = item.asin.clone();
                    let key = library.account_key.clone();
                    let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                    let author = item.author_names();
                    let narrator = item.narrator_names();
                    let cover = self.covers.get(&asin).cloned();
                    let downloaded = backend::downloaded_path(&key, &asin).is_some();
                    let active = self.download.as_ref().is_some_and(|download| download.asin == asin);
                    let current = self.player.as_ref().is_some_and(|player| player.asin == asin);
                    let action = if active {
                        div().text_size(px(14.)).text_color(color).child("Your download is in progress").into_any_element()
                    } else if current {
                        Button::new("book-open-player").label("Open player").icon(Icon::Play).variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.open_player(cx))).into_any_element()
                    } else if downloaded {
                        let book_title = title.clone();
                        Button::new("book-play").label("Play book").icon(Icon::Play).variant(ButtonVariant::Primary).on_click(cx.listener(move |this, _, _, cx| this.start_player(key.clone(), asin.clone(), book_title.clone(), cx))).into_any_element()
                    } else if self.download.is_some() {
                        div().text_size(px(14.)).text_color(color).child("Another download is in progress").into_any_element()
                    } else {
                        let book_title = title.clone();
                        Button::new("book-download").label("Download to this Mac").icon(Icon::Download).variant(ButtonVariant::Primary).on_click(cx.listener(move |this, _, _, cx| this.download(key.clone(), asin.clone(), book_title.clone(), cx))).into_any_element()
                    };
                    return div().flex().flex_col().gap(px(28.))
                        .child(Button::new("book-back").label("Back").icon(Icon::ArrowLeft).on_click(cx.listener(|this, _, _, cx| this.back(cx))))
                        .child(div().flex().items_start().gap(px(32.))
                            .child(Cover::new(cover.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(220.), 7.))
                            .child(div().flex().flex_col().gap(px(12.)).max_w(px(600.))
                                .child(div().font_family("Literata").text_size(px(34.)).child(title))
                                .child(div().text_size(px(14.)).text_color(color).child(author))
                                .children((!narrator.is_empty()).then(|| div().text_size(px(13.)).text_color(color).child(format!("Read by {narrator}"))))
                                .children(item.runtime_length_min.map(|mins| div().text_size(px(13.)).text_color(color).child(format!("{} hr {} min", mins / 60, mins % 60))))
                                .child(action)))
                        .into_any_element();
                }
                if self.page == Page::Explore {
                    return div().flex().flex_col().gap(px(18.))
                        .child(heading("Explore Audible"))
                        .child(div().flex().gap(px(12.)).child(div().w(px(400.)).child(Input::new(&self.search)))
                            .child(Button::new("search-catalog").label("Search").variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.search_catalog(cx)))) )
                        .children(self.search_message.iter().map(|message| div().text_size(px(13.)).text_color(color).child(message.clone())))
                        .children(self.search_results.iter().map(|item| {
                            let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                            let image = self.covers.get(&item.asin).cloned();
                            let product = item.clone();
                            div().id(format!("catalog-{}", item.asin)).cursor_pointer().rounded(px(8.)).hover(|style| style.bg(hover_fill)).active(|style| style.opacity(0.8)).flex().items_center().gap(px(18.)).pb(px(16.)).border_b_1().border_color(hairline)
                                .on_click(cx.listener(move |this, _, _, cx| this.open_catalog_book(product.clone(), cx)))
                                .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(72.), 6.))
                                .child(div().flex().flex_col().gap(px(5.))
                                    .child(div().font_family("Literata").text_size(px(18.)).child(title))
                                    .child(div().text_size(px(13.)).text_color(color).child(item.author_names())))
                        })).into_any_element();
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
                            .child(div().flex_1().font_family("Literata").text_size(px(18.)).child(name))
                            .child(div().text_size(px(12.)).text_color(color).child(format!("{count} {}", if count == 1 { "book" } else { "books" })))
                    })).into_any_element();
                }
                let query = self.library_search.read(cx).value().trim().to_lowercase();
                if self.page == Page::Search && query.is_empty() {
                    return base().child(Input::new(&self.library_search)).child(copy("Search the books in your library.".into())).into_any_element();
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
                        .children((self.page == Page::Search).then(|| Input::new(&self.library_search)))
                        .child(heading(match self.page { Page::Progress => "Nothing in progress", Page::OnThisMac => "No downloads yet", Page::Search => "No matching books", Page::Finished => "No finished books", _ => "Your library is empty" }))
                        .child(copy(if self.page == Page::OnThisMac { "Download a title from your library to listen on this Mac.".into() } else if self.page == Page::Search { "Try a different title, author or narrator.".into() } else if library.offline { "Showing the library saved on this Mac.".into() } else { "Your Audible library has synced.".into() })).into_any_element();
                }
                div().flex().flex_col().gap(px(16.))
                    .children((self.page == Page::Search).then(|| div().max_w(px(500.)).child(Input::new(&self.library_search))))
                    .children(matches!(self.page, Page::AuthorBooks | Page::SeriesBooks).then(|| {
                        let target = if self.page == Page::AuthorBooks { Page::Authors } else { Page::Series };
                        div().flex().items_center().gap(px(16.))
                            .child(Button::new("browse-back").label("Back").icon(Icon::ArrowLeft).on_click(cx.listener(move |this, _, _, cx| { this.page = target; cx.notify(); })))
                            .child(div().font_family("Literata").text_size(px(24.)).child(self.browse_name.clone().unwrap_or_default()))
                    }))
                    .child(div().text_size(px(13.)).text_color(color).child(format!("{} titles", items.len())))
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
                                .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(76.), 6.))
                                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(5.))
                                    .child(div().font_family("Literata").text_size(px(19.)).child(title))
                                    .child(div().text_size(px(13.)).text_color(color).child(author))))
                            .child(div().text_size(px(13.)).text_color(color).child(progress))
                            .child(if let Some(stage) = active_stage {
                                div().text_size(px(13.)).text_color(color).child(format!("{stage}…")).into_any_element()
                            } else if downloaded {
                                div().flex().items_center().gap(px(8.))
                                    .child(Button::new(format!("reveal-{asin}")).label("Show in Finder").on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(path) = &local_path && let Err(error) = std::process::Command::new("open").arg("-R").arg(path).spawn() {
                                            this.download_notice = Some(format!("Could not open Finder: {error}")); cx.notify();
                                        }
                                    })))
                                    .child(Button::new(button_id).label("Play").icon(Icon::Play).on_click(cx.listener(move |this, _, _, cx| this.start_player(key.clone(), asin.clone(), download_title.clone(), cx)))).into_any_element()
                            } else if other_download {
                                div().text_size(px(12.)).text_color(color).child("One download at a time").into_any_element()
                            } else {
                                Button::new(button_id).label("Download").on_click(cx.listener(move |this, _, _, cx| this.download(key.clone(), asin.clone(), download_title.clone(), cx))).into_any_element()
                            })
                    })).into_any_element()
            }
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.page == Page::NowPlaying && self.player.is_some() {
            return self.player_view(cx);
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
        let notice =
            if matches!(self.account, Account::Ready(_)) {
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
                                .text_size(px(13.))
                                .text_color(theme.colors.ink)
                                .child(message.clone()),
                        )
                        .child(Button::new("dismiss-status").label("Dismiss").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.download_notice = None;
                                cx.notify();
                            }),
                        ))
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
                                        .text_size(px(13.))
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(theme.accent.on_tint)
                                        .child(format!("{stage} · {title}")),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.colors.ink_secondary)
                                        .child(detail),
                                ),
                        )
                        .child(Button::new("view-downloads").label("On this Mac").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.page = Page::OnThisMac;
                                cx.notify();
                            }),
                        )),
                )
                .child(track)
                .into_any_element()
        });
        let bar =
            self.player.as_ref().map(|active| {
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
                let weak = cx.entity().downgrade();
                let scrubber = Scrubber::new("bar-seek", fraction, ScrubberColors::window(cx))
                    .on_seek(move |value, _, cx| {
                        let _ = weak.update(cx, |this, cx| this.seek_fraction(value, cx));
                    });
                div()
                    .mx(px(24.))
                    .mb(px(20.))
                    .h(px(76.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(14.))
                    .rounded(px(18.))
                    .elevation(Elevation::FloatingPlayer, &theme, window)
                    .child(
                        div()
                            .id("open-now-playing")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.open_player(cx)))
                            .w(px(260.))
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
                                    6.,
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
                                            .text_size(px(13.))
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(theme.colors.ink_secondary)
                                            .child("Open player"),
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
                                    .gap(px(20.))
                                    .child(Button::new("back-30").icon(Icon::Back30).on_click(
                                        cx.listener(|this, _, _, cx| this.skip(-30., cx)),
                                    ))
                                    .child(
                                        Button::new("toggle-play")
                                            .icon(if playing { Icon::Pause } else { Icon::Play })
                                            .variant(ButtonVariant::Ink)
                                            .shape(ButtonShape::Circle {
                                                diameter: 36.,
                                                icon: 16.,
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback(cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("forward-30").icon(Icon::Forward30).on_click(
                                            cx.listener(|this, _, _, cx| this.skip(30., cx)),
                                        ),
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
                                            .text_size(px(10.))
                                            .text_color(theme.colors.ink_secondary)
                                            .child(elapsed),
                                    )
                                    .child(div().flex_1().child(scrubber))
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(theme.colors.ink_secondary)
                                            .child(remaining),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("bar-speed")
                            .label(format!("{speed}×"))
                            .on_click(cx.listener(|this, _, _, cx| this.cycle_speed(cx))),
                    )
                    .into_any_element()
            });
        let content = div().w_full().child(self.account_view(cx));
        let content = if matches!(self.page, Page::Book | Page::CatalogBook) && !cx.reduce_motion()
        {
            gpui_kit::base::animation::EffectTransition::new(std::time::Duration::from_millis(140))
                .fade(0.92, 1.0)
                .apply(
                    content,
                    format!("book-enter-{}", self.navigation_generation),
                )
                .into_any_element()
        } else {
            content.into_any_element()
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
                    .child(self.nav("Search", Icon::Search, Page::Search, cx))
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
                            .text_size(px(12.))
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
                    .child(Toolbar::new(title))
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

fn main() {
    gpui_kit::application().with_assets(ui::Assets).run(|cx| {
        gpui_kit::init(cx);
        if let Err(error) = ui::init(backend::load_appearance(), cx) {
            eprintln!("Could not initialize the interface: {error:#}");
            cx.quit();
            return;
        }
        let bounds = Bounds::new(point(px(100.), px(100.)), size(px(1200.), px(800.)));
        match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
            Ok(_) => cx.activate(true),
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
