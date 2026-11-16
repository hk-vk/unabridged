//! Account, library and catalog loading: everything that talks to the backend
//! except playback.
use crate::*;

impl Shell {
    pub(crate) fn restore(&mut self, cx: &mut Context<Self>) {
        self.account = Account::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { backend::restore() }).await;
            let _ = this.update(cx, |this, cx| {
                this.library_fetched = Some(std::time::Instant::now());
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

    /// Re-fetches the library (progress, new books) and, when the loaded book
    /// is paused, its position, at most once a minute. Failures keep what is
    /// on screen.
    pub(crate) fn refresh_library(&mut self, cx: &mut Context<Self>) {
        // Waiting on a purchase: check whenever the user comes back.
        let min_interval = std::time::Duration::from_secs(if self.pending_purchase.is_some() {
            3
        } else {
            60
        });
        if self.refreshing
            || !matches!(self.account, Account::Ready(_))
            || self
                .library_fetched
                .is_some_and(|at| at.elapsed() < min_interval)
        {
            return;
        }
        self.refreshing = true;
        self.library_fetched = Some(std::time::Instant::now());
        let paused = self
            .player
            .as_ref()
            .filter(|active| !active.player.is_playing() && !self.loading_player)
            .map(|active| (active.account_key.clone(), active.asin.clone()));
        cx.spawn(async move |this, cx| {
            let library = cx.background_spawn(async { backend::refresh() }).await;
            let position = match paused {
                Some((key, asin)) => {
                    let found = cx
                        .background_spawn({
                            let asin = asin.clone();
                            async move { backend::position(&key, &asin) }
                        })
                        .await;
                    found.ok().flatten().map(|millis| (asin, millis))
                }
                None => None,
            };
            let _ = this.update(cx, |this, cx| {
                this.refreshing = false;
                if let Ok(Some(library)) = library
                    && !library.offline
                    && matches!(this.account, Account::Ready(_))
                {
                    let bought = this
                        .pending_purchase
                        .clone()
                        .filter(|asin| library.items.iter().any(|item| &item.asin == asin));
                    this.account = Account::Ready(library);
                    this.load_covers(cx);
                    if let Some(asin) = bought {
                        this.pending_purchase = None;
                        let showing = this.page == Page::CatalogBook
                            && this
                                .selected_catalog_book
                                .as_ref()
                                .is_some_and(|product| product.asin == asin);
                        if showing {
                            this.open_book(asin, cx);
                        }
                        this.download_notice = Some("Added to your library.".into());
                    }
                }
                if let Some((asin, millis)) = position
                    && let Some(active) = &mut this.player
                    && active.asin == asin
                    && !active.player.is_playing()
                {
                    active.player.seek(millis as f64 / 1000.);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Signs out so another Audible store can be chosen. Saves the playing
    /// position first; downloads stay on this Mac.
    pub(crate) fn change_store(&mut self, cx: &mut Context<Self>) {
        self.confirm_sign_out = false;
        if let Some(active) = &mut self.player {
            active.player.pause();
            active.was_playing = false;
        }
        self.sync_position(cx);
        self.close_mini_player(cx);
        self.player = None;
        self.read_along = None;
        self.read_along_open = false;
        self.sleep = SleepTimer::Off;
        self.pending_purchase = None;
        self.explore_feed = None;
        self.search_results.clear();
        self.page = Page::Library;
        self.account = Account::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { backend::sign_out() }).await;
            let _ = this.update(cx, |this, cx| {
                this.account = match result {
                    Ok(()) => Account::SignedOut,
                    Err(error) => Account::Error(format!("Could not sign out: {error}")),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn begin_sign_in(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn finish_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn search_catalog(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn load_explore(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn browse_genre(
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

    pub(crate) fn load_covers(&mut self, cx: &mut Context<Self>) {
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

    /// Chooses the quality for downloads from now on.
    pub(crate) fn set_download_quality(
        &mut self,
        quality: backend::DownloadQuality,
        cx: &mut Context<Self>,
    ) {
        self.prefs.download_quality = quality;
        self.save_prefs(cx);
        cx.notify();
    }

    /// The book that must not be deleted right now: the one playing, or the
    /// one downloading.
    pub(crate) fn in_use(&self, asin: &audible::Asin) -> bool {
        self.player.as_ref().is_some_and(|p| &p.asin == asin)
            || self.download.as_ref().is_some_and(|d| &d.asin == asin)
    }

    /// Deletes one downloaded book to free space.
    pub(crate) fn remove_download(&mut self, asin: audible::Asin, cx: &mut Context<Self>) {
        let Account::Ready(library) = &self.account else {
            return;
        };
        if self.in_use(&asin) {
            return;
        }
        self.download_notice = Some(
            match backend::remove_download(&library.account_key, &asin) {
                Ok(bytes) => format!(
                    "Removed the download and freed {}.",
                    backend::size_label(bytes)
                ),
                Err(error) => format!("Could not remove the download: {error}"),
            },
        );
        cx.notify();
    }

    /// Deletes every downloaded book except the one playing or downloading.
    pub(crate) fn remove_all_downloads(&mut self, cx: &mut Context<Self>) {
        self.confirm_clear_downloads = false;
        let Account::Ready(library) = &self.account else {
            return;
        };
        let key = library.account_key.clone();
        let (mut freed, mut removed, mut failed) = (0, 0, 0);
        for book in backend::downloaded_books(&key) {
            if self.in_use(&book.asin) {
                continue;
            }
            match backend::remove_download(&key, &book.asin) {
                Ok(bytes) => {
                    freed += bytes;
                    removed += 1;
                }
                Err(_) => failed += 1,
            }
        }
        self.download_notice = Some(match (removed, failed) {
            (0, 0) => "Nothing to remove.".to_string(),
            (_, 0) => format!(
                "Removed {} and freed {}.",
                model::format::plural(removed, "download"),
                backend::size_label(freed)
            ),
            _ => format!(
                "Removed {}, but {} could not be removed.",
                model::format::plural(removed, "download"),
                model::format::plural(failed, "book")
            ),
        });
        cx.notify();
    }

    pub(crate) fn download(
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
        let quality = self.prefs.download_quality;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::download(&key, asin, quality, event) })
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

    pub(crate) fn load_bookmarks(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn change_bookmark(
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

    pub(crate) fn add_current_bookmark(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &self.player {
            let asin = player.asin.clone();
            let position = (player.player.position().max(0.) * 1000.) as u64;
            self.change_bookmark(asin, position, true, cx);
        }
    }
}
