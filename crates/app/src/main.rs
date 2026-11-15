//! Better Audible's native application shell.
mod backend;
mod books;
mod library;
mod playback;
mod player;
mod region;
mod shell;
mod transcript;
mod views;
mod windows;

use books::*;
use views::{copy, heading, loading_placeholders, menus, mini_player, page_base};

use audible::{Marketplace, auth::LoginSession};
use gpui_kit::{
    AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
    component::{
        Root,
        input::{Input, InputEvent, InputState},
        select::{SearchableVec, Select, SelectState},
        shimmer::ShimmerText,
        skeleton::Skeleton,
    },
    div, linear_color_stop, linear_gradient, point,
    prelude::FluentBuilder as _,
    px, relative, size,
};
use ui::{
    ActiveTheme, Elevation, ElevationExt, Icon, TypographyExt,
    components::{
        BackLink, Button, ButtonShape, ButtonVariant, Chip, ChipStyle, Cover, CoverSize,
        IconButton, Scrubber, ScrubberColors, SearchField, SectionHeader, SegmentedProgress,
        SidebarHeader, SidebarItem, Toolbar,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Library,
    AllBooks,
    Progress,
    Explore,
    OnThisMac,
    Bookmarks,
    Search,
    Finished,
    Authors,
    Series,
    AuthorBooks,
    SeriesBooks,
    Book,
    CatalogBook,
    NowPlaying,
    Settings,
}

/// The name a back link shows for `page`.
fn page_name(page: Page) -> &'static str {
    match page {
        Page::Progress => "In progress",
        Page::Explore | Page::Search => "Explore",
        Page::OnThisMac => "On this Mac",
        Page::Bookmarks => "Bookmarks",
        Page::Finished => "Finished",
        Page::Authors | Page::AuthorBooks => "Authors",
        Page::Series | Page::SeriesBooks => "Series",
        Page::Book | Page::CatalogBook => "Book",
        Page::AllBooks => "All books",
        Page::Settings => "Settings",
        Page::Library | Page::NowPlaying => "Library",
    }
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

#[derive(Clone)]
enum SleepTimer {
    Off,
    Deadline(std::time::Instant),
    EndOfChapter {
        asin: audible::Asin,
        end_seconds: f64,
    },
}

impl SleepTimer {
    fn due(&self, now: std::time::Instant, current: Option<(&audible::Asin, f64)>) -> bool {
        match self {
            Self::Off => false,
            Self::Deadline(deadline) => now >= *deadline,
            Self::EndOfChapter { asin, end_seconds } => {
                current.is_some_and(|(current_asin, position)| {
                    current_asin == asin && position >= *end_seconds
                })
            }
        }
    }
    fn label(&self, now: std::time::Instant) -> String {
        match self {
            Self::Off => "Sleep timer off".into(),
            Self::Deadline(deadline) => format!(
                "Stop in {} min",
                deadline
                    .saturating_duration_since(now)
                    .as_secs()
                    .div_ceil(60)
            ),
            Self::EndOfChapter { .. } => "Stop after this chapter".into(),
        }
    }

    /// Text for the sleep chip: "Sleep" when off, otherwise the countdown.
    fn chip_label(&self, now: std::time::Instant) -> String {
        match self {
            Self::Off => "Sleep".into(),
            _ => self.label(now),
        }
    }
}

struct ActivePlayer {
    asin: audible::Asin,
    account_key: String,
    title: String,
    player: player::Player,
    was_playing: bool,
    chapters: Vec<audible::models::Chapter>,
    chapters_loading: bool,
}

/// The read-along transcript for the chapter on screen.
struct ReadAlong {
    asin: audible::Asin,
    start_ms: u64,
    locale: &'static str,
    job: transcript::SharedJob,
    /// The line the view last scrolled to, so it only moves on a new line.
    followed: Option<usize>,
    /// A model download is running; transcribe once it finishes.
    downloading: bool,
    /// The line that was spoken before `followed`, and when that changed,
    /// for the highlight crossfade.
    previous: Option<usize>,
    changed_at: std::time::Instant,
    /// The scroll glide in progress, if any. A `Cell` so render can finish it.
    glide: std::cell::Cell<Option<Glide>>,
}

/// An eased scroll from one offset to another.
#[derive(Clone, Copy)]
struct Glide {
    from: gpui_kit::Pixels,
    to: gpui_kit::Pixels,
    started: std::time::Instant,
}

struct Shell {
    page: Page,
    main_window: Option<gpui_kit::AnyWindowHandle>,
    mini_window: Option<gpui_kit::AnyWindowHandle>,
    /// The mini player shows time left in the book instead of the chapter.
    mini_book_time: bool,
    mini_opening: bool,
    main_opening: bool,
    previous_page: Page,
    selected_book: Option<audible::Asin>,
    selected_catalog_book: Option<audible::models::Product>,
    seek_generation: u64,
    navigation_generation: u64,
    bookmarks: Vec<(audible::Asin, u64)>,
    bookmarks_loading: bool,
    sleep: SleepTimer,
    account: Account,
    marketplace: Entity<SelectState<SearchableVec<&'static str>>>,
    suggested_marketplace: Option<Marketplace>,
    redirect: Entity<InputState>,
    search: Entity<InputState>,
    library_search: Entity<InputState>,
    browse_name: Option<String>,
    _subscriptions: Vec<gpui_kit::Subscription>,
    search_results: Vec<audible::models::Product>,
    showing_search: bool,
    explore_feed: Option<backend::ExploreFeed>,
    explore_loading: bool,
    searching: bool,
    search_message: Option<String>,
    download_notice: Option<String>,
    download: Option<ActiveDownload>,
    player: Option<ActivePlayer>,
    loading_player: bool,
    /// When the library was last fetched from Audible, to throttle refreshes.
    library_fetched: Option<std::time::Instant>,
    refreshing: bool,
    /// A catalog book sent to Audible to buy; refreshes look for it.
    pending_purchase: Option<audible::Asin>,
    read_along_open: bool,
    /// The sleep timer choice behind `sleep`, for the menu's checkmark.
    sleep_choice: &'static str,
    /// Speed and volume, saved between launches.
    prefs: backend::PlaybackPrefs,
    read_along: Option<ReadAlong>,
    read_along_scroll: gpui_kit::ScrollHandle,
    chapter_picker_scroll: gpui_kit::ScrollHandle,
    covers: std::collections::HashMap<audible::Asin, std::path::PathBuf>,
    covers_pending: std::collections::HashSet<audible::Asin>,
    palettes: std::collections::HashMap<audible::Asin, ui::theme::BookPalette>,
}

#[cfg(debug_assertions)]
fn smoke_open_mini(handle: gpui_kit::WindowHandle<Root>, cx: &mut gpui_kit::App) {
    let Some(path) = std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO") else {
        return;
    };
    let result = handle.update(cx, |root, _, cx| {
        let shell = root
            .view()
            .clone()
            .downcast::<Shell>()
            .map_err(|_| anyhow::anyhow!("Smoke test could not find the app view"))?;
        shell.update(cx, |shell, cx| {
            let player = player::Player::open(std::path::Path::new(&path))?;
            shell.player = Some(ActivePlayer {
                asin: audible::Asin::new("SMOKETEST")?,
                account_key: "0".repeat(64),
                title: "Test audio".into(),
                player,
                was_playing: false,
                chapters: Vec::new(),
                chapters_loading: false,
            });
            shell.open_player(cx);
            shell.open_mini_player(cx);
            Ok::<(), anyhow::Error>(())
        })
    });
    match result {
        Ok(Ok(())) => eprintln!("Smoke test: mini player requested"),
        Ok(Err(error)) => {
            eprintln!("Smoke test failed: {error:#}");
            cx.quit();
        }
        Err(error) => {
            eprintln!("Smoke test failed: {error:#}");
            cx.quit();
        }
    }
}

gpui_kit::actions!(
    better_audible,
    [
        /// Focuses the toolbar search field (⌘F).
        FocusSearch
    ]
);

/// The shell behind the main window, so a Dock click can bring it back.
struct MainShell(gpui_kit::WeakEntity<Shell>);

impl gpui_kit::Global for MainShell {}

/// Opens the main window with a fresh shell.
fn open_main_window(cx: &mut gpui_kit::App) {
    let bounds = Bounds::new(point(px(100.), px(100.)), size(px(1200.), px(800.)));
    match cx.open_window(
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
        |window, cx| {
            let view = cx.new(|cx| Shell::new(window, cx));
            cx.set_global(MainShell(view.downgrade()));
            cx.new(|cx| Root::new(view, window, cx))
        },
    ) {
        Ok(_handle) => {
            #[cfg(debug_assertions)]
            smoke_open_mini(_handle, cx);
            if std::env::var_os("BETTER_AUDIBLE_SMOKE_AUDIO").is_none() {
                cx.activate(true);
            }
        }
        Err(error) => {
            eprintln!("Could not open the app window: {error:#}");
            cx.quit();
        }
    }
}

/// A Dock click or relaunch while running: show the main window again. The
/// shell survives while the mini player is open; otherwise start a new one.
fn reopen(cx: &mut gpui_kit::App) {
    let shell = cx
        .try_global::<MainShell>()
        .and_then(|main| main.0.upgrade());
    match shell {
        Some(shell) => shell.update(cx, |shell, cx| shell.show_main_window(cx)),
        None => open_main_window(cx),
    }
    cx.activate(true);
}

fn main() {
    let app = gpui_kit::application().with_assets(ui::Assets);
    app.on_reopen(reopen);
    app.run(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([gpui_kit::KeyBinding::new("cmd-f", FocusSearch, None)]);
        if let Err(error) = ui::init(backend::load_appearance(), cx) {
            eprintln!("Could not initialize the interface: {error:#}");
            cx.quit();
            return;
        }
        open_main_window(cx);
    });
}

#[cfg(test)]
mod download_tests {
    use super::*;
    use audible::{DownloadEvent, download::Progress};

    #[test]
    fn sleep_timer_only_stops_at_its_deadline_or_matching_chapter_end() {
        let now = std::time::Instant::now();
        let timed = SleepTimer::Deadline(now + std::time::Duration::from_secs(30));
        assert!(!timed.due(now, None));
        assert!(timed.due(now + std::time::Duration::from_secs(30), None));
        let book = audible::Asin::new("BOOK1").unwrap();
        let other = audible::Asin::new("BOOK2").unwrap();
        let chapter = SleepTimer::EndOfChapter {
            asin: book.clone(),
            end_seconds: 120.,
        };
        assert!(!chapter.due(now, Some((&book, 119.9))));
        assert!(chapter.due(now, Some((&book, 120.))));
        assert!(!chapter.due(now, Some((&other, 120.))));
        assert!(!SleepTimer::Off.due(now, None));
        assert_eq!(
            strip_html("<p>A <b>tale</b> of&nbsp;two &amp; more.</p>"),
            "A tale of two & more."
        );
    }

    #[test]
    fn chapter_lookup_respects_boundaries_and_missing_metadata() {
        let chapters = vec![
            audible::models::Chapter {
                title: "One".into(),
                start_offset_ms: 0,
                length_ms: 1_000,
                chapters: vec![],
            },
            audible::models::Chapter {
                title: "Two".into(),
                start_offset_ms: 1_000,
                length_ms: 2_000,
                chapters: vec![],
            },
        ];
        assert_eq!(
            current_chapter(&chapters, 999).map(|(index, _)| index),
            Some(0)
        );
        assert_eq!(
            current_chapter(&chapters, 1_000).map(|(index, _)| index),
            Some(1)
        );
        assert!(current_chapter(&chapters, 3_000).is_none());
        assert!(current_chapter(&[], 0).is_none());
    }

    #[test]
    fn timeline_follows_the_current_chapter_and_maps_seeks_back_to_the_book() {
        let chapters = vec![
            audible::models::Chapter {
                title: String::new(),
                start_offset_ms: 0,
                length_ms: 100_000,
                chapters: vec![],
            },
            audible::models::Chapter {
                title: "Two".into(),
                start_offset_ms: 100_000,
                length_ms: 300_000,
                chapters: vec![],
            },
        ];
        let timeline = Timeline::chapter(&chapters, 175., 400.);
        assert_eq!(timeline.chapter_name(), Some("Two"));
        assert_eq!(timeline.elapsed().as_secs(), 75);
        assert_eq!(timeline.remaining().as_secs(), 225);
        assert_eq!(timeline.book_remaining().as_secs(), 225);
        assert!((timeline.fraction() - 0.25).abs() < 1e-6);
        // Half way through chapter two is 250 s into a 400 s book.
        assert!((timeline.book_fraction(0.5) - 0.625).abs() < 1e-6);
        assert_eq!(
            Timeline::chapter(&chapters, 10., 400.).chapter_name(),
            Some("Chapter 1")
        );
        // Without chapters the span is the whole book.
        let book = Timeline::chapter(&[], 100., 400.);
        assert!(book.chapter.is_none());
        assert!((book.fraction() - 0.25).abs() < 1e-6);
        assert!((book.book_fraction(0.5) - 0.5).abs() < 1e-6);
    }

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
