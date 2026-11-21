//! Settings: appearance, account and storage.
use super::*;
use ui::components::Switch;

impl Shell {
    pub(crate) fn apply_appearance(&mut self, settings: ui::ThemeSettings, cx: &mut Context<Self>) {
        ui::Theme::apply_settings(settings, cx);
        crate::sync_app_icon(cx.window_appearance(), cx);
        let version = backend::register_appearance_write();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { backend::save_appearance(settings, version) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.download_notice = Some(format!("Could not save appearance: {error}"));
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn settings_view(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let settings = theme.settings;
        let modes = ui::theme::Mode::ALL
            .into_iter()
            .map(|mode| {
                let selected = settings.mode == mode;
                Button::new(format!("appearance-mode-{}", mode.label()))
                    .label(mode.label())
                    .variant(if selected {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut settings = cx.theme().settings;
                        settings.mode = mode;
                        this.apply_appearance(settings, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let icons = ui::theme::Mode::ALL
            .into_iter()
            .map(|mode| {
                let selected = settings.app_icon == mode;
                Button::new(format!("app-icon-{}", mode.label()))
                    .label(mode.label())
                    .variant(if selected {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut settings = cx.theme().settings;
                        settings.app_icon = mode;
                        this.apply_appearance(settings, cx);
                    }))
            })
            .collect::<Vec<_>>();
        let accents = ui::theme::AccentTheme::ALL
            .into_iter()
            .map(|accent| {
                let selected = settings.accent == accent;
                let color =
                    ui::theme::Accent::resolve(accent, theme.appearance, theme.colors.window);
                div()
                    .id(format!("appearance-accent-{}", accent.name()))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(10.))
                    .bg(if selected {
                        theme.accent.tint
                    } else {
                        theme.colors.fill
                    })
                    .hover(|style| style.opacity(0.85))
                    .active(|style| style.opacity(0.72))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut settings = cx.theme().settings;
                        settings.accent = accent;
                        this.apply_appearance(settings, cx);
                    }))
                    .child(div().size(px(24.)).rounded_full().bg(color.fill))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .type_style(ui::text::CALLOUT)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(accent.name()),
                            )
                            .child(
                                div()
                                    .type_style(ui::text::CAPTION)
                                    .text_color(theme.colors.ink_secondary)
                                    .child(accent.note()),
                            ),
                    )
                    .children(selected.then(|| {
                        ui::icon(Icon::Tick)
                            .size(px(16.))
                            .text_color(theme.accent.text)
                    }))
            })
            .collect::<Vec<_>>();
        let weak = cx.entity().downgrade();
        let toggle =
            Switch::new("cover-tint", settings.tint_with_cover).on_toggle(move |on, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    let mut settings = cx.theme().settings;
                    settings.tint_with_cover = on;
                    this.apply_appearance(settings, cx);
                });
            });
        let appearance = div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(div().type_style(ui::text::BODY_MEDIUM).child("Mode"))
                    .child(div().flex().gap(px(10.)).children(modes)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(div().type_style(ui::text::BODY_MEDIUM).child("App icon"))
                    .child(div().flex().gap(px(10.)).children(icons)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(div().type_style(ui::text::BODY_MEDIUM).child("Accent theme"))
                    .children(accents),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .type_style(ui::text::CALLOUT)
                                    .child("Tint Now Playing with the cover"),
                            )
                            .child(
                                div()
                                    .type_style(ui::text::CAPTION)
                                    .text_color(theme.colors.ink_secondary)
                                    .child(
                                        "The player takes its colour from the book you are listening to.",
                                    ),
                            ),
                    )
                    .child(toggle),
            );
        div()
            .max_w(px(640.))
            .flex()
            .flex_col()
            .gap(px(32.))
            .child(settings_section(
                Icon::PaintBoard,
                "Appearance",
                appearance,
                cx,
            ))
            .children(self.account_settings(cx))
            .children(self.storage_settings(cx))
            .child(self.about_settings(cx))
            .into_any_element()
    }

    fn account_settings(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let Account::Ready(library) = &self.account else {
            return None;
        };
        let theme = cx.theme().clone();
        let status = if library.offline {
            "Offline. Showing the library saved on this Mac."
        } else {
            "Connected. Progress syncs with your other devices."
        };
        let marketplace = library.marketplace.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(!self.confirm_sign_out, |row| {
                row.child(
                    Button::new("settings-refresh")
                        .label(if self.refreshing {
                            "Refreshing…"
                        } else {
                            "Refresh library"
                        })
                        .icon(Icon::Refresh)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.library_fetched = None;
                            this.refresh_library(cx);
                        })),
                )
                .child(
                    Button::new("settings-change-store")
                        .label("Sign out")
                        .icon(Icon::Logout)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_sign_out = true;
                            cx.notify();
                        })),
                )
            })
            .when(self.confirm_sign_out, |row| {
                row.child(
                    Button::new("settings-cancel-sign-out")
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_sign_out = false;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("settings-confirm-sign-out")
                        .label("Sign out")
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|this, _, _, cx| this.change_store(cx))),
                )
            });
        let caption = if self.confirm_sign_out {
            format!(
                "Sign out of Audible {marketplace}? You can sign in again with any store. Downloads stay on this Mac."
            )
        } else {
            status.to_string()
        };
        let row = setting_row(format!("Audible {marketplace}"), caption, actions, &theme);
        Some(settings_section(Icon::UserCircle, "Account", row, cx))
    }

    /// The version, and "Check for Updates…" in release builds.
    fn about_settings(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let updates = crate::UPDATES_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed);
        let caption = if updates {
            "Checks for updates once a day and installs them when you agree."
        } else {
            "This copy was built locally, so it doesn’t update itself."
        };
        let action = div().children(updates.then(|| {
            Button::new("check-for-updates")
                .label("Check for Updates…")
                .icon(Icon::Refresh)
                .on_click(|_, _, _| crate::check_for_updates())
        }));
        let version_row = setting_row(
            format!("Unabridged {}", env!("CARGO_PKG_VERSION")),
            caption.into(),
            action,
            &theme,
        );
        // A standing invitation, never a prompt: it only shows here.
        let star_row = setting_row(
            "Enjoying Unabridged?".into(),
            "A star on GitHub helps other Mac listeners find it.".into(),
            Button::new("star-on-github")
                .label("Star on GitHub")
                .icon(Icon::External)
                .on_click(|_, _, cx| cx.open_url(crate::REPOSITORY_URL)),
            &theme,
        );
        let content = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(version_row)
            .child(div().h(px(1.)).bg(theme.colors.hairline))
            .child(star_row);
        settings_section(Icon::Settings, "About", content, cx)
    }

    fn storage_settings(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let Account::Ready(library) = &self.account else {
            return None;
        };
        let theme = cx.theme().clone();
        let directory = backend::books_directory(&library.account_key)?;

        // Download quality: applies to new downloads.
        let current = self.prefs.download_quality;
        let qualities = backend::DownloadQuality::ALL
            .into_iter()
            .map(|quality| {
                Button::new(format!("download-quality-{}", quality.label()))
                    .label(quality.label())
                    .variant(if quality == current {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.set_download_quality(quality, cx)),
                    )
            })
            .collect::<Vec<_>>();
        let quality_caption = match current {
            backend::DownloadQuality::High => {
                "Best sound, about 55 MB per hour of listening. Applies to new downloads."
            }
            backend::DownloadQuality::Standard => {
                "Half the size, about 28 MB per hour. Voices still sound clear. Applies to new downloads."
            }
        };
        let quality_row = setting_row(
            "Download quality".into(),
            quality_caption.into(),
            div().flex().gap(px(8.)).children(qualities),
            &theme,
        );

        // What is downloaded, and how to free the space.
        let books = backend::downloaded_books(&library.account_key);
        let total: u64 = books.iter().map(|book| book.bytes).sum();
        let removable = books.iter().filter(|book| !self.in_use(&book.asin)).count();
        let summary = match books.len() {
            0 => "No books downloaded yet.".to_string(),
            1 => format!("1 book, {} on this Mac.", backend::size_label(total)),
            count => format!("{count} books, {} on this Mac.", backend::size_label(total)),
        };
        let caption = if self.confirm_clear_downloads {
            let kept = books.len() - removable;
            format!(
                "Remove {} from this Mac?{} Your progress and bookmarks stay, and you can download them again.",
                model::format::plural(removable as u64, "book"),
                if kept > 0 {
                    " The book you’re listening to stays."
                } else {
                    ""
                }
            )
        } else {
            summary
        };
        let actions = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(!self.confirm_clear_downloads, |row| {
                row.child(
                    Button::new("settings-reveal")
                        .label(crate::file_browser::SHOW_LABEL)
                        .icon(Icon::Folder)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = std::fs::create_dir_all(&directory);
                            if let Err(error) = crate::file_browser::open(&directory, false) {
                                this.download_notice = Some(format!(
                                    "Could not open {}: {error}",
                                    crate::file_browser::NAME
                                ));
                                cx.notify();
                            }
                        })),
                )
                .when(removable > 0, |row| {
                    row.child(
                        Button::new("settings-clear-downloads")
                            .label("Remove all")
                            .icon(Icon::Delete)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_clear_downloads = true;
                                cx.notify();
                            })),
                    )
                })
            })
            .when(self.confirm_clear_downloads, |row| {
                row.child(
                    Button::new("settings-cancel-clear")
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_clear_downloads = false;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("settings-confirm-clear")
                        .label("Remove")
                        .variant(ButtonVariant::Primary)
                        .on_click(cx.listener(|this, _, _, cx| this.remove_all_downloads(cx))),
                )
            });
        let downloads_row = setting_row("Downloaded books".into(), caption, actions, &theme);

        // One line per book, biggest first, each removable on its own.
        let rows =
            books.iter().map(|book| {
                let title = library
                    .items
                    .iter()
                    .find(|item| item.asin == book.asin)
                    .and_then(|item| item.product.title.clone())
                    .unwrap_or_else(|| book.asin.to_string());
                let in_use = self.in_use(&book.asin);
                let asin = book.asin.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(30.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .type_style(ui::text::BODY)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .type_style(ui::text::CAPTION.tabular())
                            .text_color(theme.colors.ink_secondary)
                            .child(if in_use {
                                format!("{} · in use", backend::size_label(book.bytes))
                            } else {
                                backend::size_label(book.bytes)
                            }),
                    )
                    .child(div().flex_none().w(px(30.)).when(!in_use, |slot| {
                        slot.child(
                            IconButton::new(
                                format!("remove-download-{}", book.asin),
                                Icon::Delete,
                                16.,
                            )
                            .hit_area(30.)
                            .color(theme.colors.ink_secondary)
                            .hover_fill(theme.colors.fill)
                            .tooltip("Remove download")
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.remove_download(asin.clone(), cx),
                            )),
                        )
                    }))
            });

        let content = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(quality_row)
            .child(div().h(px(1.)).bg(theme.colors.hairline))
            .child(downloads_row)
            .when(!books.is_empty(), |content| {
                content.child(div().flex().flex_col().gap(px(2.)).children(rows))
            });
        Some(settings_section(Icon::HardDrive, "Storage", content, cx))
    }
}

/// A titled settings group: an icon and title over a rounded card.
fn settings_section(
    icon: Icon,
    title: &'static str,
    content: impl IntoElement,
    cx: &Context<Shell>,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::icon(icon).size(px(18.)).text_color(theme.accent.text))
                .child(div().type_style(ui::text::GROUP_TITLE).child(title)),
        )
        .child(
            div()
                .p(px(18.))
                .rounded(px(ui::metrics::radius::WINDOW))
                .bg(theme.colors.fill)
                .child(content),
        )
        .into_any_element()
}

/// A label with a caption on the left and an action on the right.
fn setting_row(
    title: String,
    caption: String,
    action: impl IntoElement,
    theme: &ui::Theme,
) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().type_style(ui::text::CALLOUT).child(title))
                .child(
                    div()
                        .type_style(ui::text::CAPTION)
                        .text_color(theme.colors.ink_secondary)
                        .child(caption),
                ),
        )
        .child(action)
}
