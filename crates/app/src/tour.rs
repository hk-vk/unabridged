//! A scripted walk through the app for memory measurements (debug builds only).
//!
//! `BETTER_AUDIBLE_TOUR=1` visits each page and then plays a silent file from
//! `BETTER_AUDIBLE_TOUR_AUDIO` in the player and the mini player. It prints a
//! `TOUR <state>` line once each state has settled, so a script can read the
//! process footprint at that moment (see `tools/measure-memory.sh`).
use crate::*;

const SETTLE: std::time::Duration = std::time::Duration::from_secs(4);

pub(crate) fn start(handle: gpui_kit::WindowHandle<Root>, cx: &mut gpui_kit::App) {
    if std::env::var_os("BETTER_AUDIBLE_TOUR").is_none() {
        return;
    }
    // GPUI stops drawing a fully covered window, and covers load only when
    // drawn: keep the window on top but invisible and click-through.
    let shell = handle.update(cx, |root, window, _| {
        ghost(window);
        root.view().clone().downcast::<Shell>().ok()
    });
    let Ok(Some(shell)) = shell else {
        eprintln!("TOUR failed: no app view");
        return;
    };
    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let wait = |seconds: u64| executor.timer(std::time::Duration::from_secs(seconds));
        // Wait for the library and its covers, as a user would after launch.
        for _ in 0..120 {
            wait(1).await;
            let ready = shell.update(cx, |shell, _| {
                matches!(shell.account, Account::Ready(_)) && shell.covers_pending.is_empty()
            });
            if ready {
                break;
            }
        }
        wait(SETTLE.as_secs() * 2).await;
        let (ready, books, covers) = shell.update(cx, |shell, _| match &shell.account {
            Account::Ready(library) => (true, library.items.len(), shell.covers.len()),
            _ => (false, 0, shell.covers.len()),
        });
        eprintln!("tour: ready {ready}, {books} books, {covers} covers");
        println!("TOUR launch");

        let pages = [
            (Page::AllBooks, "all-books"),
            (Page::Progress, "in-progress"),
            (Page::Explore, "explore"),
            (Page::Finished, "finished"),
            (Page::Authors, "authors"),
            (Page::Series, "series"),
            (Page::Settings, "settings"),
            (Page::Library, "home"),
        ];
        for (page, name) in pages {
            shell.update(cx, |shell, cx| {
                shell.page = page;
                if page == Page::Explore {
                    shell.load_explore(cx);
                }
                cx.notify();
            });
            wait(if page == Page::Explore {
                10
            } else {
                SETTLE.as_secs()
            })
            .await;
            println!("TOUR page {name}");
        }
        wait(10).await;
        println!("TOUR idle-after-pages");

        let Some(audio) = std::env::var_os("BETTER_AUDIBLE_TOUR_AUDIO") else {
            println!("TOUR done");
            return;
        };
        let started = shell.update(cx, |shell, cx| {
            let asin = audible::Asin::new("SMOKETEST")?;
            if let Some(cover) = shell.covers.values().next().cloned() {
                shell.covers.insert(asin.clone(), cover);
            }
            let player = player::Player::open(std::path::Path::new(&audio))?;
            player.play()?;
            shell.player = Some(ActivePlayer {
                asin,
                account_key: "0".repeat(64),
                title: "Test audio".into(),
                player,
                was_playing: true,
                chapters: Vec::new(),
                chapters_loading: false,
            });
            shell.open_player(cx);
            Ok::<_, anyhow::Error>(())
        });
        if let Err(error) = started {
            println!("TOUR failed: {error:#}");
            return;
        }
        wait(15).await;
        println!("TOUR playing");
        shell.update(cx, |shell, cx| shell.open_mini_player(cx));
        wait(1).await;
        let mini = shell.update(cx, |shell, _| shell.mini_window);
        if let Some(mini) = mini {
            let _ = mini.update(cx, |_, window, _| ghost(window));
        }
        wait(9).await;
        println!("TOUR mini-player");
        shell.update(cx, |shell, cx| {
            if let Some(active) = &mut shell.player {
                active.player.pause();
                active.was_playing = false;
            }
            cx.notify();
        });
        wait(10).await;
        println!("TOUR done");
    })
    .detach();
}

fn ghost(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    unsafe extern "C" {
        fn ba_window_ghost(view: *mut std::ffi::c_void);
    }
    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
    {
        // SAFETY: `ns_view` is the live content view of this window, on the
        // main thread.
        unsafe { ba_window_ghost(appkit.ns_view.as_ptr()) }
    }
}
