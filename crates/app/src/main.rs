//! Better Audible's native application shell.
mod backend;
mod player;

use audible::{Marketplace, auth::LoginSession};
use gpui_kit::{
    AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
    component::{
        Root,
        input::{Input, InputState},
        select::{SearchableVec, Select, SelectState},
    },
    div, point, px, relative, size,
};
use ui::{
    ActiveTheme, Elevation, ElevationExt, Icon, ThemeSettings,
    components::{
        Button, ButtonShape, ButtonVariant, Cover, CoverSize, SidebarHeader, SidebarItem, Toolbar,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Library,
    Progress,
    Explore,
    OnThisMac,
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

struct ActivePlayer {
    asin: audible::Asin,
    account_key: String,
    title: String,
    player: player::Player,
    was_playing: bool,
}

struct Shell {
    page: Page,
    account: Account,
    marketplace: Entity<SelectState<SearchableVec<&'static str>>>,
    redirect: Entity<InputState>,
    search: Entity<InputState>,
    search_results: Vec<audible::models::Product>,
    searching: bool,
    search_message: Option<String>,
    download_notice: Option<String>,
    download: Option<ActiveDownload>,
    player: Option<ActivePlayer>,
    loading_player: bool,
    covers: std::collections::HashMap<audible::Asin, std::path::PathBuf>,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let redirect =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste the final URL here"));
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search titles, authors or narrators")
        });
        let marketplaces = SearchableVec::new(
            Marketplace::ALL
                .iter()
                .map(|m| m.display_name())
                .collect::<Vec<_>>(),
        );
        let marketplace = cx.new(|cx| SelectState::new(marketplaces, None, window, cx));
        let mut shell = Self {
            page: Page::Library,
            account: Account::Loading,
            marketplace,
            redirect,
            search,
            search_results: Vec::new(),
            searching: false,
            search_message: None,
            download_notice: None,
            download: None,
            player: None,
            loading_player: false,
            covers: Default::default(),
        };
        shell.restore(cx);
        cx.spawn(async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.download.is_some() {
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
                            if ticks >= 30 {
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
                    .background_spawn(async move { backend::cover(&asin, &url) })
                    .await;
                if let Ok(path) = result
                    && this
                        .update(cx, |this, cx| {
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
        if self.loading_player {
            return;
        }
        if self.player.as_ref().is_some_and(|p| p.asin == asin) {
            if let Some(active) = &mut self.player {
                if let Err(error) = active.player.play() {
                    self.download_notice = Some(format!("Could not resume playback: {error}"));
                } else {
                    active.was_playing = true;
                }
            }
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
            let result = cx
                .background_spawn({
                    let asin = asin.clone();
                    {
                        let key = key.clone();
                        async move { backend::position(&key, &asin) }
                    }
                })
                .await;
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
                        this.player = Some(ActivePlayer {
                            asin,
                            account_key: key,
                            title,
                            player,
                            was_playing: true,
                        });
                        this.download_notice = None;
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

    fn account_view(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let color = cx.theme().colors.ink_secondary;
        let hairline = cx.theme().colors.hairline;
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
                if self.page == Page::Explore {
                    return div().flex().flex_col().gap(px(18.))
                        .child(heading("Explore Audible"))
                        .child(div().flex().gap(px(12.)).child(div().w(px(400.)).child(Input::new(&self.search)))
                            .child(Button::new("search-catalog").label("Search").variant(ButtonVariant::Primary).on_click(cx.listener(|this, _, _, cx| this.search_catalog(cx)))) )
                        .children(self.search_message.iter().map(|message| div().text_size(px(13.)).text_color(color).child(message.clone())))
                        .children(self.search_results.iter().map(|item| {
                            let title = item.title.clone().unwrap_or_else(|| "Untitled".into());
                            let image = self.covers.get(&item.asin).cloned();
                            div().flex().items_center().gap(px(18.)).pb(px(16.)).border_b_1().border_color(hairline)
                                .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(72.), 6.))
                                .child(div().flex().flex_col().gap(px(5.))
                                    .child(div().font_family("Literata").text_size(px(18.)).child(title))
                                    .child(div().text_size(px(13.)).text_color(color).child(item.author_names())))
                        })).into_any_element();
                }
                let items: Vec<_> = library.items.iter().filter(|item| match self.page {
                    Page::Progress => item.percent_complete.unwrap_or(0.) > 0. && item.is_finished != Some(true),
                    Page::OnThisMac => backend::downloaded_path(&library.account_key, &item.asin).is_some() || self.download.as_ref().is_some_and(|download| download.asin == item.asin),
                    _ => true,
                }).collect();
                if items.is_empty() {
                    return base().child(heading(match self.page { Page::Progress => "Nothing in progress", Page::OnThisMac => "No downloads yet", _ => "Your library is empty" }))
                        .child(copy(if self.page == Page::OnThisMac { "Download a title from your library to listen on this Mac.".into() } else { "Your Audible library has synced.".into() })).into_any_element();
                }
                div().flex().flex_col().gap(px(16.))
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
                        let progress = item.percent_complete.map(|v| format!("{v:.0}% complete")).unwrap_or_else(|| "Not started".into());
                        div().flex().items_center().gap(px(20.)).pb(px(16.)).border_b_1().border_color(hairline)
                            .child(Cover::new(image.map(gpui_kit::ImageSource::from).unwrap_or_else(|| gpui_kit::ImageSource::from("")), CoverSize::Square(76.), 6.))
                            .child(div().flex_1().flex().flex_col().gap(px(5.))
                                .child(div().font_family("Literata").text_size(px(19.)).child(title))
                                .child(div().text_size(px(13.)).text_color(color).child(author)))
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
        let theme = cx.theme().clone();
        let title = match self.page {
            Page::Library => "Library",
            Page::Progress => "In progress",
            Page::Explore => "Explore",
            Page::OnThisMac => "On this Mac",
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
        let bar = self.player.as_ref().map(|active| {
            let title = active.title.clone();
            let playing = active.player.is_playing();
            let position = active.player.position().max(0.) as u64;
            let duration = active.player.duration().max(0.) as u64;
            let time = format!(
                "{} / {}",
                model::format::clock(std::time::Duration::from_secs(position)),
                model::format::clock(std::time::Duration::from_secs(duration))
            );
            div()
                .mx(px(24.))
                .mb(px(20.))
                .h(px(76.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(16.))
                .px(px(18.))
                .rounded(px(18.))
                .elevation(Elevation::FloatingPlayer, &theme, window)
                .child(
                    div()
                        .w(px(250.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().text_size(px(13.)).child(title))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.colors.ink_secondary)
                                .child(time),
                        ),
                )
                .child(div().flex_1())
                .child(
                    Button::new("back-30")
                        .icon(Icon::Back30)
                        .on_click(cx.listener(|this, _, _, cx| this.skip(-30., cx))),
                )
                .child(
                    Button::new("toggle-play")
                        .icon(if playing { Icon::Pause } else { Icon::Play })
                        .variant(ButtonVariant::Ink)
                        .shape(ButtonShape::Circle {
                            diameter: 36.,
                            icon: 16.,
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_playback(cx))),
                )
                .child(
                    Button::new("forward-30")
                        .icon(Icon::Forward30)
                        .on_click(cx.listener(|this, _, _, cx| this.skip(30., cx))),
                )
                .child(div().flex_1())
                .into_any_element()
        });
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
                    .child(self.nav("On this Mac", Icon::Download, Page::OnThisMac, cx))
                    .child(div().flex_1())
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
                            .child(self.account_view(cx)),
                    )
                    .children(bar),
            )
    }
}

fn main() {
    gpui_kit::application().with_assets(ui::Assets).run(|cx| {
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
