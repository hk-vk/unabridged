use crate::*;
use gpui_kit::WindowControlArea;

pub(crate) fn frame(
    content: gpui_kit::AnyElement,
    window: &Window,
    cx: &gpui_kit::App,
    ink: gpui_kit::Hsla,
) -> gpui_kit::AnyElement {
    static FONT: std::sync::OnceLock<Option<&str>> = std::sync::OnceLock::new();
    let font = *FONT.get_or_init(|| {
        let fonts = cx.text_system().all_font_names();
        ["Segoe Fluent Icons", "Segoe MDL2 Assets"]
            .into_iter()
            .find(|candidate| fonts.iter().any(|name| name == candidate))
    });
    let colors = cx.theme().colors;
    let control = |id, glyph, fallback, area| {
        div()
            .id(id)
            .w(px(46.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(ink)
            .text_size(px(if font.is_some() { 10. } else { 16. }))
            .font_family(font.unwrap_or(ui::typography::family::UI))
            .window_control_area(area)
            .hover(move |style| {
                if area == WindowControlArea::Close {
                    style
                        .bg(gpui_kit::rgb(0xc42b1c))
                        .text_color(gpui_kit::rgb(0xffffff))
                } else {
                    style.bg(colors.fill)
                }
            })
            .child(if font.is_some() { glyph } else { fallback })
    };
    div()
        .size_full()
        .relative()
        .child(content)
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .flex()
                .occlude()
                .child(control(
                    "window-minimize",
                    "\u{e921}",
                    "−",
                    WindowControlArea::Min,
                ))
                .child(control(
                    "window-maximize",
                    if window.is_maximized() {
                        "\u{e923}"
                    } else {
                        "\u{e922}"
                    },
                    if window.is_maximized() { "❐" } else { "□" },
                    WindowControlArea::Max,
                ))
                .child(control(
                    "window-close",
                    "\u{e8bb}",
                    "×",
                    WindowControlArea::Close,
                )),
        )
        .into_any_element()
}
