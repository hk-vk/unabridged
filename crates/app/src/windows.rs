//! The extra windows: the floating mini player and reopening the main window.
use crate::*;

impl Shell {
    pub(crate) fn open_mini_player(&mut self, cx: &mut Context<Self>) {
        if self.player.is_none() || self.mini_opening {
            return;
        }
        self.mini_opening = true;
        let shell = cx.entity();
        let existing = self.mini_window;
        // Opening a window can render immediately. Defer it until the Shell
        // entity is no longer mutably borrowed by the click handler.
        cx.defer(move |cx| {
            if let Some(handle) = existing
                && handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok()
            {
                shell.update(cx, |this, cx| {
                    this.mini_opening = false;
                    cx.notify();
                });
                return;
            }
            let bounds = Bounds::new(
                point(px(160.), px(160.)),
                size(px(mini_player::SIZE.0), px(mini_player::SIZE.1)),
            );
            let view_shell = shell.clone();
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        appears_transparent: cfg!(target_os = "macos"),
                        ..Default::default()
                    }),
                    kind: gpui_kit::WindowKind::Floating,
                    window_background: gpui_kit::WindowBackgroundAppearance::Blurred,
                    show: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    focus: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    is_resizable: false,
                    ..Default::default()
                },
                |window, cx| {
                    mini_player::float_everywhere(window);
                    let mini = cx.new(|cx| mini_player::MiniPlayer::new(view_shell, cx));
                    cx.new(|cx| Root::new(mini, window, cx))
                },
            );
            // The panel may not be attached to its view until after creation;
            // apply the native tweaks again now that it is (they are idempotent).
            if let Ok(handle) = &result {
                let _ = handle.update(cx, |_, window, _| mini_player::float_everywhere(window));
            }
            shell.update(cx, |this, cx| {
                this.mini_opening = false;
                match result {
                    Ok(handle) => {
                        this.mini_window = Some(handle.into());
                        #[cfg(debug_assertions)]
                        if std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_some() {
                            eprintln!("Smoke test: mini player window opened");
                        }
                    }
                    Err(error) => {
                        this.download_notice =
                            Some(format!("Could not open the mini player: {error}"))
                    }
                }
                cx.notify();
            });
            #[cfg(debug_assertions)]
            if std::env::var_os("BETTER_AUDIBLE_SMOKE_REOPEN").is_some() {
                let shell = shell.clone();
                cx.defer(move |cx| {
                    let main = shell.read(cx).main_window;
                    if let Some(handle) = main {
                        let _ = handle.update(cx, |_, window, _| window.remove_window());
                    }
                    shell.update(cx, |this, cx| this.show_main_player(cx));
                });
            }
        });
    }

    pub(crate) fn close_mini_player(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.mini_window.take() {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
        cx.notify();
    }

    pub(crate) fn show_main_player(&mut self, cx: &mut Context<Self>) {
        self.open_player(cx);
        self.show_main_window(cx);
    }

    /// Brings the main window forward, reopening it on the current page if
    /// it was closed (for example after a click on the Dock icon).
    pub(crate) fn show_main_window(&mut self, cx: &mut Context<Self>) {
        if self.main_opening {
            return;
        }
        self.main_opening = true;
        let shell = cx.entity();
        let existing = self.main_window;
        cx.defer(move |cx| {
            if let Some(handle) = existing
                && handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok()
            {
                shell.update(cx, |this, cx| {
                    this.main_opening = false;
                    cx.notify();
                });
                return;
            }
            let bounds = Bounds::new(point(px(100.), px(100.)), size(px(1200.), px(800.)));
            let view_shell = shell.clone();
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    focus: std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none(),
                    titlebar: Some(TitlebarOptions {
                        appears_transparent: cfg!(target_os = "macos"),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Root::new(view_shell, window, cx)),
            );
            shell.update(cx, |this, cx| {
                this.main_opening = false;
                match result {
                    Ok(handle) => {
                        this.main_window = Some(handle.into());
                        #[cfg(debug_assertions)]
                        if std::env::var_os("BETTER_AUDIBLE_SMOKE_REOPEN").is_some() {
                            eprintln!("Smoke test: main player window reopened");
                        }
                    }
                    Err(error) => {
                        this.download_notice =
                            Some(format!("Could not open the main window: {error}"))
                    }
                }
                cx.notify();
            });
        });
    }
}
