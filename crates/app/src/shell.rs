//! Shell setup and navigation between pages.
use crate::*;

impl Shell {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search the Audible catalog"));
        let library_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search titles, authors or narrators")
        });
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, window, cx| {
                ui::Theme::sync_system_appearance(window.appearance(), cx);
                sync_app_icon(window.appearance(), cx);
            }),
            // Coming back to the app picks up listening done on other devices
            // and books added since launch.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.refresh_library(cx);
                }
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
            bookmarks: Vec::new(),
            bookmarks_loading: false,
            sleep: SleepTimer::Off,
            account: Account::Loading,
            marketplace,
            suggested_marketplace,
            sign_in_note: None,
            sign_in_watch: 0,
            sign_in_clipboard: None,
            connecting_after_sign_in: false,
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
            library_fetched: None,
            refreshing: false,
            pending_purchase: None,
            confirm_sign_out: false,
            confirm_clear_downloads: false,
            read_along_open: false,
            sleep_choice: "Off",
            prefs: backend::load_playback(),
            read_along: None,
            read_along_follow: Default::default(),
            mini_follow: Default::default(),
            mini_lyrics: false,
            mini_lyrics_from: 0.,
            mini_lyrics_at: None,
            chapter_picker_scroll: Default::default(),
            covers: Default::default(),
            covers_pending: Default::default(),
            palettes: Default::default(),
        };
        shell.restore(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            let mut shown_second = None;
            let mut shown_buffering = false;
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
                                // A stream that lost its connection stops too.
                                if let Some(error) = player.player.error() {
                                    this.download_notice = Some(if player.is_streaming() {
                                        format!("Playback stopped: {error}. Check your connection, or download the book to listen offline.")
                                    } else {
                                        format!("Playback stopped: {error}")
                                    });
                                }
                            }
                            this.sync_position(cx);
                            cx.notify();
                        }
                        if (this.read_along_open && this.page == Page::NowPlaying)
                            || (this.mini_lyrics && this.mini_window.is_some())
                        {
                            this.ensure_read_along(cx);
                            this.follow_read_along(cx.reduce_motion());
                            cx.notify();
                        }
                        this.recover_stream(cx);
                        let buffering = this.player.as_ref().is_some_and(|p| p.player.is_buffering());
                        if buffering != shown_buffering {
                            shown_buffering = buffering;
                            cx.notify();
                        }
                        if let Some(active) = this.player.as_ref().filter(|p| p.player.is_playing())
                        {
                            ticks += 1;
                            if ticks >= 150 {
                                this.sync_position(cx);
                                ticks = 0;
                            }
                            // Times show whole seconds: redraw once a second, not
                            // on every tick. Each redraw repaints the whole window.
                            let second = active.player.position().max(0.) as u64;
                            if shown_second != Some(second) {
                                shown_second = Some(second);
                                cx.notify();
                            }
                        } else {
                            shown_second = None;
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

    pub(crate) fn open_book(&mut self, asin: audible::Asin, cx: &mut Context<Self>) {
        self.selected_book = Some(asin);
        if self.page != Page::Book {
            self.previous_page = self.page;
        }
        self.page = Page::Book;
        self.navigation_generation += 1;
        cx.notify();
    }

    pub(crate) fn open_catalog_book(
        &mut self,
        product: audible::models::Product,
        cx: &mut Context<Self>,
    ) {
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

    pub(crate) fn open_player(&mut self, cx: &mut Context<Self>) {
        if self.player.is_some() {
            if self.page != Page::NowPlaying {
                self.previous_page = self.page;
            }
            self.page = Page::NowPlaying;
            self.navigation_generation += 1;
            cx.notify();
        }
    }

    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
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

    /// Where the back link goes, named for the destination.
    pub(crate) fn back_label(&self) -> &'static str {
        let destination = if self.previous_page == self.page {
            Page::Library
        } else {
            self.previous_page
        };
        page_name(destination)
    }
}
