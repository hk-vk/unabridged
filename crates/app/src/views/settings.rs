//! Settings: appearance, account and storage.
use super::*;
use ui::components::Switch;

impl Shell {
    fn apply_appearance(&mut self, settings: ui::ThemeSettings, cx: &mut Context<Self>) {
        ui::Theme::apply_settings(settings, cx);
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
        let row = setting_row(
            format!("Audible {}", library.marketplace),
            status.to_string(),
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
            &theme,
        );
        Some(settings_section(Icon::UserCircle, "Account", row, cx))
    }

    fn storage_settings(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let Account::Ready(library) = &self.account else {
            return None;
        };
        let theme = cx.theme().clone();
        let directory = backend::books_directory(&library.account_key)?;
        let (count, bytes) = std::fs::read_dir(&directory)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .path()
                            .extension()
                            .is_some_and(|extension| extension == "m4b" || extension == "m4a")
                    })
                    .fold((0_usize, 0_u64), |(count, bytes), entry| {
                        (
                            count + 1,
                            bytes + entry.metadata().map(|m| m.len()).unwrap_or(0),
                        )
                    })
            })
            .unwrap_or((0, 0));
        let summary = match count {
            0 => "No books downloaded yet.".to_string(),
            1 => format!("1 book, {}", size_label(bytes)),
            _ => format!("{count} books, {}", size_label(bytes)),
        };
        let row = setting_row(
            "Downloaded books".into(),
            summary,
            Button::new("settings-reveal")
                .label("Show in Finder")
                .icon(Icon::Folder)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let _ = std::fs::create_dir_all(&directory);
                    if let Err(error) = std::process::Command::new("open").arg(&directory).spawn() {
                        this.download_notice = Some(format!("Could not open Finder: {error}"));
                        cx.notify();
                    }
                })),
            &theme,
        );
        Some(settings_section(Icon::HardDrive, "Storage", row, cx))
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

fn size_label(bytes: u64) -> String {
    const GB: f64 = 1024. * 1024. * 1024.;
    const MB: f64 = 1024. * 1024.;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}
