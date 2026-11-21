use crate::*;
use gpui_kit::WindowControlArea;

pub(crate) const HEIGHT: f32 = 32.;

pub(crate) fn frame(
    content: gpui_kit::AnyElement,
    window: &Window,
    cx: &gpui_kit::App,
    mini: bool,
) -> gpui_kit::AnyElement {
    let colors = cx.theme().colors;
    let control = |id, label, area| {
        div()
            .id(id)
            .w(px(46.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
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
            .child(label)
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(colors.window)
        .text_color(colors.ink)
        .font_family(ui::typography::family::UI)
        .child(
            div()
                .h(px(HEIGHT))
                .flex_none()
                .flex()
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .pl(px(12.))
                        .flex()
                        .items_center()
                        .window_control_area(WindowControlArea::Drag)
                        .child("Unabridged"),
                )
                .when(!mini, |bar| {
                    bar.child(control("window-minimize", "−", WindowControlArea::Min))
                        .child(control(
                            "window-maximize",
                            if window.is_maximized() { "❐" } else { "□" },
                            WindowControlArea::Max,
                        ))
                })
                .child(control("window-close", "×", WindowControlArea::Close)),
        )
        .child(div().flex_1().min_h_0().w_full().child(content))
        .into_any_element()
}
