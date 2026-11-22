//! The main window frame: sidebar, toolbar, notices, floating player bar.
use crate::*;

impl Shell {
    pub(crate) fn nav(
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

    fn notice_view(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme().clone();
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
        }
    }

    fn download_panel(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme().clone();
        self.download.as_ref().map(|download| {
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
        })
    }

    fn floating_bar(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme().clone();
        self.player.as_ref().map(|active| {
            let compact_bar = f32::from(window.viewport_size().width) < 1050.;
            let title = active.title.clone();
            let image = self.covers.get(&active.asin).cloned();
            let playing = active.player.is_playing();
            // Time and scrubber follow the current chapter, like the full player.
            let timeline = active.timeline();
            let fraction = timeline.fraction();
            let elapsed = model::format::clock(timeline.elapsed());
            let remaining = model::format::countdown(timeline.remaining());
            let speed = active.player.rate();
            let subtitle = if active.player.is_buffering() {
                "Buffering…".to_string()
            } else {
                timeline.chapter_name().unwrap_or("Now playing").to_string()
            };
            let download = active
                .is_streaming()
                .then(|| {
                    self.download_icon("bar", &active.account_key, &active.asin, &active.title, cx)
                })
                .flatten();
            let sleeping = !matches!(self.sleep, SleepTimer::Off);
            let sleep_tip = if sleeping {
                self.sleep.label(std::time::Instant::now())
            } else {
                "Sleep timer".into()
            };
            let weak = cx.entity().downgrade();
            let scrubber = Scrubber::new("bar-seek", fraction, ScrubberColors::window(cx)).on_seek(
                move |value, _, cx| {
                    let target = timeline.book_fraction(value);
                    let _ = weak.update(cx, |this, cx| this.seek_fraction(target, cx));
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
                        .child(self.speed_picker(
                            "bar-speed-menu",
                            gpui_kit::Anchor::BottomRight,
                            menus::PickerColors::window(cx),
                            Chip::new("bar-speed").label(model::format::speed(speed)),
                            cx,
                        ))
                        .child(
                            self.sleep_picker(
                                "bar-sleep-menu",
                                gpui_kit::Anchor::BottomRight,
                                menus::PickerColors::window(cx),
                                IconButton::new("bar-sleep", Icon::Moon, 16.)
                                    .hit_area(28.)
                                    .color(if sleeping {
                                        theme.accent.text
                                    } else {
                                        theme.colors.ink_secondary
                                    })
                                    .hover_fill(theme.colors.fill)
                                    .tooltip(sleep_tip),
                                cx,
                            ),
                        )
                        .child(
                            self.chapter_picker(
                                "bar-chapters-menu",
                                gpui_kit::Anchor::BottomRight,
                                menus::PickerColors::window(cx),
                                IconButton::new("bar-chapters", Icon::Chapters, 16.)
                                    .hit_area(28.)
                                    .hover_fill(theme.colors.fill)
                                    .tooltip("Chapters"),
                                cx,
                            ),
                        )
                        .children(download)
                        .children((!compact_bar).then(|| {
                            IconButton::new("bar-mini", Icon::Pip, 16.)
                                .hit_area(28.)
                                .hover_fill(theme.colors.fill)
                                .tooltip("Open mini player")
                                .on_click(cx.listener(|this, _, _, cx| this.open_mini_player(cx)))
                        })),
                )
                .into_any_element()
        })
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        div()
            .w(px(220.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .px(px(12.))
            .pt(px(if cfg!(target_os = "macos") {
                58.
            } else if cfg!(target_os = "windows") {
                52.
            } else {
                16.
            }))
            .relative()
            .when(cfg!(target_os = "windows"), |sidebar| {
                sidebar.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(px(52.))
                        .window_control_area(gpui_kit::WindowControlArea::Drag),
                )
            })
            .border_r_1()
            .border_color(theme.colors.hairline)
            .bg(theme.colors.sidebar)
            .child(self.nav("Explore", Icon::Compass, Page::Explore, cx))
            .child(div().h(px(24.)))
            .child(SidebarHeader::new("Your library"))
            .child(self.nav("Library", Icon::Library, Page::Library, cx))
            .child(self.nav("In progress", Icon::Clock, Page::Progress, cx))
            .child(self.nav(LOCAL_BOOKS_LABEL, Icon::Download, Page::OnThisMac, cx))
            .child(self.nav("Bookmarks", Icon::Bookmark, Page::Bookmarks, cx))
            .child(div().h(px(20.)))
            .child(SidebarHeader::new("Browse"))
            .child(self.nav("Authors", Icon::User, Page::Authors, cx))
            .child(self.nav("Series", Icon::Layers, Page::Series, cx))
            .child(self.nav("Finished", Icon::CheckmarkCircle, Page::Finished, cx))
            .child(div().flex_1())
            .child(self.nav("Settings", Icon::Settings, Page::Settings, cx))
            .child(div().h(px(18.)))
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_frame(window, cx);
        #[cfg(target_os = "windows")]
        {
            let ink = if self.page == Page::NowPlaying {
                cx.theme()
                    .book_palette(
                        self.player
                            .as_ref()
                            .and_then(|active| self.palettes.get(&active.asin).copied()),
                    )
                    .on_field
            } else {
                cx.theme().colors.ink
            };
            crate::chrome::frame(content, window, cx, ink)
        }
        #[cfg(not(target_os = "windows"))]
        content
    }
}

impl Shell {
    fn render_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        if self.page == Page::NowPlaying && self.player.is_some() {
            return self.player_view(window, cx);
        }
        let theme = cx.theme().clone();
        let title = match self.page {
            Page::Library => "Library",
            Page::AllBooks => "All books",
            Page::Progress => "In progress",
            Page::Explore => "Explore",
            Page::OnThisMac => LOCAL_BOOKS_LABEL,
            Page::Bookmarks => "Bookmarks",
            Page::Search => "Search",
            Page::Finished => "Finished",
            Page::Authors | Page::AuthorBooks => "Authors",
            Page::Series | Page::SeriesBooks => "Series",
            Page::Book | Page::CatalogBook => "Book",
            Page::NowPlaying => "Now playing",
            Page::Settings => "Settings",
        };
        let notice = self.notice_view(cx);
        let download_panel = self.download_panel(cx);
        let bar = self.floating_bar(window, cx);
        let content = div().w_full().child(self.content_view(window, cx));
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
            .on_action(
                cx.listener(|this, _: &ReadAlongLarger, _, cx| this.step_read_along_size(1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ReadAlongSmaller, _, cx| this.step_read_along_size(-1, cx)),
            )
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
            .font_family(ui::typography::family::UI)
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .when(cfg!(target_os = "windows"), |header| {
                                header
                                    .pr(px(138.))
                                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                            })
                            .child(
                                Toolbar::new(title).trailing(
                                    div()
                                        .when(cfg!(target_os = "windows"), |search| {
                                            search.occlude()
                                        })
                                        .child(
                                            if matches!(
                                                self.page,
                                                Page::Explore | Page::CatalogBook
                                            ) {
                                                SearchField::new(
                                                    &self.search,
                                                    if cfg!(target_os = "macos") {
                                                        "⌘F"
                                                    } else {
                                                        "Ctrl+F"
                                                    },
                                                )
                                            } else {
                                                SearchField::new(
                                                    &self.library_search,
                                                    if cfg!(target_os = "macos") {
                                                        "⌘F"
                                                    } else {
                                                        "Ctrl+F"
                                                    },
                                                )
                                            },
                                        ),
                                ),
                            ),
                    )
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
