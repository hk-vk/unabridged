//! Streaming books that aren't on this Mac.
//!
//! A stream licenses the book like a download does, then fetches and
//! decrypts only the parts the player (or read along) asks for. Those parts
//! are cached under `~/Library/Caches/Better Audible/Streams/<account>/`, so
//! going back over a stretch, or reopening a recent book, doesn't fetch it
//! again. Only the few most recently streamed books are kept.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use audible::{Asin, Audible, AudibleClient, StreamSource};

/// How many books keep their stream cache.
const KEEP_BOOKS: usize = 3;

/// The open stream for each cache file. Two streams on one file would undo
/// each other's writes (a fresh one empties the file the other is filling),
/// so a book being opened again, while it still loads or while read along
/// holds it, gets the stream already open.
type OpenSlot = Arc<Mutex<Weak<Stream>>>;
static OPEN: Mutex<Option<HashMap<PathBuf, OpenSlot>>> = Mutex::new(None);

/// An open stream for one book.
pub struct Stream {
    source: StreamSource,
    client: Audible,
    asin: Asin,
    quality: audible::models::Quality,
    runtime: tokio::runtime::Runtime,
    /// One re-license at a time when the download link expires.
    relicense: std::sync::Mutex<()>,
}

impl Stream {
    /// Licenses `asin` and reads the start of its file, or returns the stream
    /// already open for it. Blocking: run it on a background thread.
    pub fn open(
        key: &str,
        asin: &Asin,
        quality: crate::backend::DownloadQuality,
    ) -> anyhow::Result<Arc<Self>> {
        let directory = cache_directory(key)?;
        let cache = directory.join(cache_name(asin, quality));
        let slot = {
            let mut open = OPEN
                .lock()
                .map_err(|_| anyhow::anyhow!("Stream lock poisoned"))?;
            let open = open.get_or_insert_with(HashMap::new);
            // Forget slots whose stream has gone, unless one is opening.
            open.retain(|_, slot| {
                Arc::strong_count(slot) > 1
                    || slot
                        .try_lock()
                        .map_or(true, |stream| stream.strong_count() > 0)
            });
            open.entry(cache.clone()).or_default().clone()
        };
        let mut current = slot
            .lock()
            .map_err(|_| anyhow::anyhow!("Stream lock poisoned"))?;
        if let Some(stream) = current.upgrade() {
            touch(&cache);
            return Ok(stream);
        }
        let stream = Self::open_at(&directory, cache, asin, quality)?;
        *current = Arc::downgrade(&stream);
        Ok(stream)
    }

    fn open_at(
        directory: &Path,
        cache: PathBuf,
        asin: &Asin,
        quality: crate::backend::DownloadQuality,
    ) -> anyhow::Result<Arc<Self>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("stream")
            .enable_all()
            .build()?;
        let _guard = runtime.enter();
        let client = crate::backend::client()?;
        let quality = quality.audible();
        std::fs::create_dir_all(directory)?;
        let had_cache = cache
            .with_file_name(format!("{}.ranges", cache_file_name(&cache)))
            .exists();
        let source = runtime.block_on(async {
            let license = client.license(asin, quality).await?;
            let http = client.http().reqwest().clone();
            match StreamSource::open(http.clone(), &license, cache.clone()).await {
                // A cache that no longer reads as audio (damaged, say, by a
                // crash) would fail the same way on every retry: start it
                // over once.
                Err(audible::Error::InvalidMedia(_) | audible::Error::Crypto(_)) if had_cache => {
                    remove_cache(&cache);
                    StreamSource::open(http, &license, cache.clone()).await
                }
                result => result,
            }
        })?;
        touch(&cache);
        prune(directory, &cache);
        Ok(Arc::new(Self {
            source,
            client,
            asin: asin.clone(),
            quality,
            runtime,
            relicense: std::sync::Mutex::new(()),
        }))
    }

    pub fn len(&self) -> u64 {
        self.source.len()
    }

    pub fn content_type(&self) -> &'static str {
        self.source.content_type()
    }

    pub fn duration(&self) -> Option<f64> {
        self.source.duration()
    }

    /// The cache file: a playable copy of the book wherever it's been
    /// fetched, for read along.
    pub fn cache_path(&self) -> &Path {
        self.source.cache_path()
    }

    /// Fetches book time `start..end` into the cache. Blocking.
    pub fn prefetch(
        &self,
        start: f64,
        end: f64,
        progress: &(dyn Fn(f32) + Send + Sync),
    ) -> anyhow::Result<()> {
        self.with_fresh_url(|| {
            self.runtime
                .block_on(self.source.prefetch_time(start, end, progress))
        })
    }

    /// Runs `attempt`, and once more after a new license if Audible's
    /// download link expired in between.
    fn with_fresh_url<T>(&self, attempt: impl Fn() -> audible::Result<T>) -> anyhow::Result<T> {
        match attempt() {
            Err(error) if is_expired(&error) => {
                {
                    let _one = self
                        .relicense
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Stream lock poisoned"))?;
                    let license = self
                        .runtime
                        .block_on(self.client.license(&self.asin, self.quality))?;
                    self.source.set_url(license.download_url);
                }
                Ok(attempt()?)
            }
            result => Ok(result?),
        }
    }
}

impl crate::player::StreamRead for Stream {
    fn read_at(
        &self,
        offset: u64,
        buf: &mut [u8],
        closed: &dyn Fn() -> bool,
    ) -> anyhow::Result<usize> {
        let bytes = self.with_fresh_url(|| {
            self.runtime.block_on(async {
                // A closed player's reads stop (dropping a read part-way is
                // safe) instead of tying up a thread for minutes on a bad
                // network.
                let read = std::pin::pin!(self.source.read(offset, buf.len()));
                let closing = std::pin::pin!(async {
                    while !closed() {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                });
                match futures_select(read, closing).await {
                    Some(result) => result,
                    None => Err(audible::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Playback was closed",
                    ))),
                }
            })
        })?;
        let n = bytes.len().min(buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Ok(n)
    }
}

/// `<asin>-<quality>.m4b`: each quality is a different file, so a cache is
/// never filled with two qualities' bytes.
fn cache_name(asin: &Asin, quality: crate::backend::DownloadQuality) -> String {
    format!("{asin}-{}.m4b", quality.label().to_ascii_lowercase())
}

fn cache_file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The first of `read` and `closing` to finish: `Some` with the read's
/// result, or `None` if closing won.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
async fn futures_select<T>(
    mut read: std::pin::Pin<&mut impl std::future::Future<Output = T>>,
    mut closing: std::pin::Pin<&mut impl std::future::Future<Output = ()>>,
) -> Option<T> {
    std::future::poll_fn(|cx| {
        if let std::task::Poll::Ready(value) = read.as_mut().poll(cx) {
            return std::task::Poll::Ready(Some(value));
        }
        closing.as_mut().poll(cx).map(|()| None)
    })
    .await
}

fn is_expired(error: &audible::Error) -> bool {
    matches!(error, audible::Error::UrlExpired { .. })
}

fn cache_directory(key: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid account identifier"
    );
    let home =
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
    Ok(PathBuf::from(home)
        .join("Library/Caches/Better Audible/Streams")
        .join(key))
}

/// Marks a cache as just used, for [`prune`].
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

/// Keeps the [`KEEP_BOOKS`] most recently streamed books' caches, with their
/// sidecar files.
fn prune(directory: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut books: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "m4b") && path != keep)
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    books.sort_by_key(|book| std::cmp::Reverse(book.0));
    for (_, path) in books.into_iter().skip(KEEP_BOOKS.saturating_sub(1)) {
        remove_cache(&path);
    }
}

/// Deletes a book's stream cache and its sidecars (`<asin>.m4b.*`).
fn remove_cache(path: &Path) {
    let _ = std::fs::remove_file(path);
    let (Some(directory), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    if let Ok(entries) = std::fs::read_dir(directory) {
        let prefix = format!("{name}.");
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&prefix))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Drops a book's stream cache once it's downloaded: the download plays
/// instead.
pub fn forget(key: &str, asin: &Asin) {
    if let Ok(directory) = cache_directory(key) {
        // The name before qualities were kept apart, too.
        remove_cache(&directory.join(format!("{asin}.m4b")));
        for quality in crate::backend::DownloadQuality::ALL {
            remove_cache(&directory.join(cache_name(asin, quality)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Streams part of a real book from the signed-in account, and
    /// transcribes a stretch of the stream cache the way read along does:
    /// `BA_KEY=<account> BA_ASIN=<asin> cargo test -p better-audible real_book -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a signed-in account and the network"]
    fn streams_a_real_book() {
        let key = std::env::var("BA_KEY").unwrap();
        let asin = Asin::new(std::env::var("BA_ASIN").unwrap()).unwrap();
        let started = std::time::Instant::now();
        let stream = Stream::open(&key, &asin, crate::backend::DownloadQuality::High).unwrap();
        println!(
            "opened in {:?}: {} bytes, {:?} s, {}",
            started.elapsed(),
            stream.len(),
            stream.duration(),
            stream.content_type()
        );
        let mut buf = vec![0u8; 256 * 1024];
        let middle = stream.len() / 2;
        let started = std::time::Instant::now();
        let n = crate::player::StreamRead::read_at(&*stream, middle, &mut buf, &|| false).unwrap();
        println!("read {n} bytes mid-book in {:?}", started.elapsed());
        assert_eq!(n, buf.len());
        let started = std::time::Instant::now();
        stream.prefetch(300., 330., &|_| {}).unwrap();
        println!("fetched 300–330 s in {:?}", started.elapsed());
        let helper = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("ba-transcribe");
        let output = std::process::Command::new(helper)
            .args(["transcribe"])
            .arg(stream.cache_path())
            .args(["300", "330", "en-US"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        println!("{text}");
        assert!(text.contains("\"text\""), "no transcript lines");
    }

    #[test]
    fn a_read_stops_once_its_player_closes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let closed = std::sync::atomic::AtomicBool::new(false);
        let started = std::time::Instant::now();
        let result = runtime.block_on(async {
            // A read stuck on the network.
            let read = std::pin::pin!(std::future::pending::<u8>());
            let closing = std::pin::pin!(async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                closed.store(true, std::sync::atomic::Ordering::Relaxed);
            });
            futures_select(read, closing).await
        });
        assert_eq!(result, None);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        // A read that finishes first wins.
        let result = runtime.block_on(futures_select(
            std::pin::pin!(async { 7 }),
            std::pin::pin!(std::future::pending::<()>()),
        ));
        assert_eq!(result, Some(7));
    }

    #[test]
    fn each_quality_has_its_own_cache_file() {
        let asin = Asin::new("B0G3CHBFWR").unwrap();
        assert_eq!(
            cache_name(&asin, crate::backend::DownloadQuality::High),
            "B0G3CHBFWR-high.m4b"
        );
        assert_eq!(
            cache_name(&asin, crate::backend::DownloadQuality::Standard),
            "B0G3CHBFWR-standard.m4b"
        );
    }

    #[test]
    fn prune_keeps_the_most_recent_books_and_their_sidecars() {
        let dir = std::env::temp_dir().join(format!("ba-stream-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = std::time::SystemTime::now();
        for (i, name) in ["A", "B", "C", "D"].iter().enumerate() {
            let path = dir.join(format!("{name}.m4b"));
            std::fs::write(&path, b"x").unwrap();
            std::fs::write(dir.join(format!("{name}.m4b.ranges")), b"[]").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(now - std::time::Duration::from_secs(100 * i as u64))
                .unwrap();
        }
        // D is the oldest but the one being opened.
        prune(&dir, &dir.join("D.m4b"));
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "A.m4b",
                "A.m4b.ranges",
                "B.m4b",
                "B.m4b.ranges",
                "D.m4b",
                "D.m4b.ranges"
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
