//! Appearance settings and persistence.
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

    pub(super) fn settings_view(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
                                    .text_size(px(14.))
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(accent.name()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
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
        div().max_w(px(640.)).flex().flex_col().gap(px(24.))
            .child(div().flex().flex_col().gap(px(10.))
                .child(div().text_size(px(16.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("Mode"))
                .child(div().flex().gap(px(10.)).children(modes)))
            .child(div().flex().flex_col().gap(px(10.))
                .child(div().text_size(px(16.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child("Accent theme"))
                .children(accents))
            .child(div().flex().items_center().gap(px(14.)).py(px(10.))
                .child(div().flex_1().flex().flex_col().gap(px(3.))
                    .child(div().text_size(px(14.)).child("Tint Now Playing with the cover"))
                    .child(div().text_size(px(12.)).text_color(theme.colors.ink_secondary).child("The player takes its colour from the book you are listening to.")))
                .child(toggle))
            .into_any_element()
    }
}
