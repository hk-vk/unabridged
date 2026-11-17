//! Cover images decoded at the size they are drawn, and freed once off screen.
//!
//! GPUI's default loader keeps every image it has ever drawn, at full size, in
//! memory and in each window's GPU atlas until the app quits. Covers come in
//! at 500 px but are mostly drawn at 48–88 pt, so this cache decodes each
//! cover at its drawn pixel size and drops it a little while after its window
//! stops drawing it.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{AnyWindowHandle, App, Global, ImageCacheError, RenderImage, Window, WindowId};

/// How long a cover stays after its window last drew it, so going back and
/// forth between pages does not decode it again.
const KEEP: Duration = Duration::from_secs(20);
/// How often unused covers are looked for.
const SWEEP_EVERY: Duration = Duration::from_secs(5);

type Loaded = Result<Arc<RenderImage>, ImageCacheError>;

struct Entry {
    image: Option<Loaded>,
    used: Instant,
}

struct WindowCovers {
    handle: AnyWindowHandle,
    entries: HashMap<(PathBuf, u32), Entry>,
    /// When this window last drew a cover.
    drawn: Instant,
}

#[derive(Default)]
struct CoverCache {
    windows: HashMap<WindowId, WindowCovers>,
    sweeping: bool,
}

impl Global for CoverCache {}

/// The cover at `path`, decoded to fit `pixels` square (0 keeps its own size).
/// `None` while it is still loading; the window redraws once it is ready.
pub(crate) fn load(path: &Path, pixels: u32, window: &mut Window, cx: &mut App) -> Option<Loaded> {
    let handle = window.window_handle();
    let id = handle.window_id();
    let now = Instant::now();
    let key = (path.to_path_buf(), pixels);
    let cache = cx.default_global::<CoverCache>();
    let covers = cache.windows.entry(id).or_insert_with(|| WindowCovers {
        handle,
        entries: HashMap::new(),
        drawn: now,
    });
    covers.drawn = now;
    if let Some(entry) = covers.entries.get_mut(&key) {
        entry.used = now;
        return entry.image.clone();
    }
    covers.entries.insert(
        key.clone(),
        Entry {
            image: None,
            used: now,
        },
    );
    let start_sweep = !std::mem::replace(&mut cache.sweeping, true);

    let decode = cx
        .background_executor()
        .spawn(async move { decode(&key.0, key.1).map(Arc::new) });
    let path = path.to_path_buf();
    cx.spawn(async move |cx| {
        let image = decode
            .await
            .map_err(|error| ImageCacheError::Other(Arc::new(error)));
        cx.update(|cx| {
            let cache = cx.default_global::<CoverCache>();
            if let Some(entry) = cache
                .windows
                .get_mut(&id)
                .and_then(|covers| covers.entries.get_mut(&(path, pixels)))
            {
                entry.image = Some(image);
            }
            let _ = handle.update(cx, |_, window, _| window.refresh());
        });
    })
    .detach();

    if start_sweep {
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(SWEEP_EVERY).await;
                if !cx.update(sweep) {
                    break;
                }
            }
        })
        .detach();
    }
    None
}

/// Drops covers no window has drawn for a while. Returns whether there is
/// anything left to watch.
fn sweep(cx: &mut App) -> bool {
    let open = cx.windows();
    let now = Instant::now();
    let cache = cx.default_global::<CoverCache>();
    // A closed window's atlas is gone with it; its decoded images go here.
    let mut freed = cache.windows.len();
    cache.windows.retain(|_, covers| open.contains(&covers.handle));
    freed -= cache.windows.len();

    let mut drops = Vec::new();
    for covers in cache.windows.values_mut() {
        let drawn = covers.drawn;
        let mut images = Vec::new();
        covers.entries.retain(|_, entry| {
            // Never drop what the window's latest frame drew; that frame may be
            // shown again without a new layout.
            let stale = entry.image.is_some()
                && entry.used + Duration::from_secs(1) < drawn
                && now.duration_since(entry.used) > KEEP;
            if stale && let Some(Ok(image)) = entry.image.take() {
                images.push(image);
            }
            !stale
        });
        if !images.is_empty() {
            drops.push((covers.handle, images));
        }
    }
    let done = cache.windows.is_empty();
    if done {
        cache.sweeping = false;
    }

    for (handle, images) in drops {
        freed += images.len();
        let _ = handle.update(cx, |_, window, _| {
            for image in images {
                let _ = window.drop_image(image);
            }
        });
    }
    if freed > 0 {
        release_free_memory();
    }
    !done
}

fn decode(path: &Path, pixels: u32) -> anyhow::Result<RenderImage> {
    let image = image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?;
    let image = if pixels > 0 && pixels < image.width().max(image.height()) {
        image.resize(pixels, pixels, image::imageops::FilterType::CatmullRom)
    } else {
        image
    };
    let mut data = image.into_rgba8();
    // GPUI draws BGRA.
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(RenderImage::new(vec![image::Frame::new(data)]))
}

/// Hands memory freed by dropped covers back to macOS now, rather than when
/// the system next asks for it.
fn release_free_memory() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone means all zones; a goal of 0 releases what it can.
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
}
