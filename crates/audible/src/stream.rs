//! Streaming playback: serve a decrypted, playable M4B byte stream for a
//! licensed book straight from Audible's CDN, without downloading it first.
//!
//! [`StreamSource::open`] fetches the file's top-level box headers and its
//! `moov` with HTTP `Range` requests and parses the sample tables. After that
//! any byte range can be read: missing bytes are fetched on demand (widened to
//! whole encrypted samples, so every sample is decrypted from its first byte
//! with the IV), decrypted, patched exactly like [`crate::drm`]'s converter
//! (`aavd` → `mp4a`, DRM boxes → `free`, `aax*` brands → `M4B `) and written to
//! a sparse cache file at their real offsets. Because every patch keeps its
//! size, no offset moves: once every byte is cached, the cache file *is* the
//! playable M4B.
//!
//! The cache file `<name>` has a sidecar `<name>.ranges` (JSON, written
//! atomically) listing the byte ranges present, the file length and a hash of
//! the key. Reopening a book reuses the cache only when both match.

use std::fs::{File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{RANGE, RETRY_AFTER, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::api::content::{ContentDrm, License};
use crate::credentials::Secret;
use crate::crypto::aes128_cbc_decrypt_blocks;
use crate::download::{DOWNLOAD_USER_AGENT, parse_content_range};
use crate::drm::DecryptionKey;
use crate::drm::convert::track_key;
use crate::drm::mp4::{self, Layout};
use crate::error::{Error, Result};
use crate::http::{RetryPolicy, map_status};

/// Bump when the cache layout or the patching rules change.
const SIDECAR_VERSION: u32 = 1;
/// Prefetch spans closer than this are fetched as one span.
const MERGE_GAP: u64 = 64 * 1024;
/// Files with more top-level boxes than this are not Audible files.
const MAX_TOP_BOXES: usize = 4096;
/// Largest `ftyp` box we accept.
const MAX_FTYP: u64 = 64 * 1024;

/// UTI for MPEG-4 audio, as AVFoundation expects it.
const UTI_M4A: &str = "com.apple.m4a-audio";
/// UTI for MP3.
const UTI_MP3: &str = "public.mp3";

fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        file.read_exact_at(buf, offset)
    }
    #[cfg(windows)]
    {
        read_exact_at_with(buf, offset, |buf, offset| file.seek_read(buf, offset))
    }
}

fn write_all_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        file.write_all_at(buf, offset)
    }
    #[cfg(windows)]
    {
        write_all_at_with(buf, offset, |buf, offset| file.seek_write(buf, offset))
    }
}

#[cfg(any(windows, test))]
fn read_exact_at_with(
    mut buf: &mut [u8],
    mut offset: u64,
    mut read: impl FnMut(&mut [u8], u64) -> io::Result<usize>,
) -> io::Result<()> {
    while !buf.is_empty() {
        match read(buf, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn write_all_at_with(
    mut buf: &[u8],
    mut offset: u64,
    mut write: impl FnMut(&[u8], u64) -> io::Result<usize>,
) -> io::Result<()> {
    while !buf.is_empty() {
        match write(buf, offset) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                buf = &buf[n..];
                offset += n as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Tuning for [`StreamSource`]. The defaults suit playback; tests use tiny
/// sizes to exercise partial fetches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamOptions {
    /// Bytes requested from the start of the file when opening. Covers
    /// `ftyp` and the `moov` header of a typical file.
    pub probe_len: u64,
    /// Smallest request for missing bytes, so sequential playback is not
    /// hundreds of tiny requests. Never re-fetches cached bytes to reach it.
    pub min_fetch: u64,
    /// Largest single request.
    pub max_fetch: u64,
    /// Seconds of audio added on both sides of a [`StreamSource::prefetch_time`]
    /// window (AAC decoder pre-roll).
    pub prefetch_margin: f64,
    /// Request size for [`StreamSource::prefetch_time`]: small, so each
    /// request finishes well within `request_timeout` even on a slow
    /// connection.
    pub prefetch_chunk: u64,
    /// How many [`prefetch_time`](StreamSource::prefetch_time) requests run
    /// at once, so small requests still fill a fast connection.
    pub prefetch_parallel: usize,
    /// Retries for transient failures (network errors, 5xx, 429).
    pub retry: RetryPolicy,
    /// Timeout for a single request, body included.
    pub request_timeout: Duration,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            probe_len: 128 * 1024,
            min_fetch: 512 * 1024,
            max_fetch: 4 * 1024 * 1024,
            prefetch_margin: 3.0,
            prefetch_chunk: 512 * 1024,
            prefetch_parallel: 4,
            retry: RetryPolicy {
                max_attempts: 4,
                base_delay: Duration::from_millis(300),
                max_delay: Duration::from_secs(5),
            },
            request_timeout: Duration::from_secs(60),
        }
    }
}

/// A seekable, decrypted view of a licensed book on the CDN, backed by a
/// sparse on-disk cache. `Send + Sync`; share it as `Arc<StreamSource>` and
/// call [`read`](Self::read) from as many threads as you like.
pub struct StreamSource {
    fetcher: Fetcher,
    options: StreamOptions,
    len: u64,
    format: Format,
    codec: Codec,
    duration: Option<f64>,
    cache_path: PathBuf,
    sidecar_path: PathBuf,
    fingerprint: String,
    /// The cache file. Positional I/O (`read_at`/`write_at`) needs no lock:
    /// bytes are written before their range is recorded, and recorded
    /// ranges are never removed.
    file: File,
    state: Mutex<State>,
    /// Signalled whenever an in-flight fetch finishes (or fails).
    fetched: Notify,
}

impl std::fmt::Debug for StreamSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamSource")
            .field("len", &self.len)
            .field("content_type", &self.content_type())
            .field("cache_path", &self.cache_path)
            .field("cached_fraction", &self.cached_fraction())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
enum Format {
    Mp4,
    /// MP3 with the offset where audio frames start and an estimated
    /// duration (only used to place prefetch windows).
    Mp3 {
        data_start: u64,
        estimate: Option<f64>,
    },
}

/// A same-size byte patch: four bytes written at an absolute offset.
type Patch = (u64, [u8; 4]);

#[derive(Debug, Default)]
struct State {
    ranges: RangeSet,
    /// Ranges being fetched right now, so concurrent readers wait instead of
    /// fetching the same bytes twice.
    inflight: Vec<(u64, u64)>,
    /// Original bytes under each patch, persisted so a reopened stream can
    /// read the raw `moov` back from the (patched) cache.
    originals: Vec<Patch>,
}

impl StreamSource {
    /// Open a stream for a granted license with default [`StreamOptions`].
    ///
    /// Fetches the top-level box headers and the whole `moov` (+ `ftyp`)
    /// with Range requests, parses them and writes the patched head into
    /// `cache_path` (a sparse file holding decrypted bytes at their real
    /// offsets; its parent directory is created). A previous cache for the
    /// same file and key is reused; when the CDN cannot be reached (offline,
    /// or the URL expired) a previous cache is opened on its own, so cached
    /// parts stay playable.
    ///
    /// Handles AAXC and AAX keys and DRM-free MP4/MP3 files. A rejected URL
    /// yields [`Error::UrlExpired`].
    pub async fn open(
        client: reqwest::Client,
        license: &License,
        cache_path: PathBuf,
    ) -> Result<Self> {
        Self::open_with(client, license, cache_path, StreamOptions::default()).await
    }

    /// [`open`](Self::open) with explicit tuning.
    pub async fn open_with(
        client: reqwest::Client,
        license: &License,
        cache_path: PathBuf,
        options: StreamOptions,
    ) -> Result<Self> {
        if options.min_fetch == 0 || options.max_fetch == 0 || options.probe_len < 16 {
            return Err(Error::InvalidArgument(
                "stream fetch sizes must be positive and the probe at least 16 bytes".into(),
            ));
        }
        let key = match &license.drm {
            ContentDrm::Aaxc(k) => Some(k),
            ContentDrm::None => None,
        };
        let fingerprint = key_fingerprint(key);
        let sidecar_path = sibling(&cache_path, ".ranges");
        if let Some(dir) = cache_path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let prior = Sidecar::load(&sidecar_path)
            .filter(|s| s.version == SIDECAR_VERSION && s.key == fingerprint);

        let fetcher = Fetcher {
            client,
            url: RwLock::new(license.download_url.clone()),
        };
        // With a cache to fall back on, do not spend seconds retrying.
        let attempts = if prior.is_some() {
            1
        } else {
            options.retry.max_attempts
        };
        let (len, first, offline) = match fetcher
            .get(0, options.probe_len, None, &options, attempts)
            .await
        {
            Ok((bytes, len)) => (len, Some(bytes), None),
            Err(e) => match &prior {
                Some(p) => {
                    warn!(error = %e, "stream URL unusable; opening from the cache");
                    (p.len, None, Some(e))
                }
                None => return Err(e),
            },
        };
        if len == 0 {
            return Err(Error::InvalidMedia("the remote file is empty".into()));
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&cache_path)?;
        let file_len = file.metadata()?.len();
        let prior = prior.filter(|p| p.len == len && file_len == len);
        let (ranges, originals) = match prior {
            Some(p) => {
                debug!(path = %cache_path.display(), "reusing stream cache");
                (RangeSet::from_vec(p.ranges), p.originals)
            }
            None => {
                if let Some(e) = offline {
                    return Err(e);
                }
                file.set_len(0)?;
                file.set_len(len)?;
                debug!(path = %cache_path.display(), len, "new stream cache");
                (RangeSet::default(), Vec::new())
            }
        };

        let mut head = HeadReader {
            fetcher: &fetcher,
            options: &options,
            len,
            bufs: first.map(|b| vec![(0, b)]).unwrap_or_default(),
            file: &file,
            ranges: &ranges,
            originals: &originals,
            offline,
        };
        let magic = head.read(0, 16).await?;
        let (format, codec, duration) = if is_mp3(&magic) {
            let data_start = id3_len(&magic).min(len);
            let frames = head.read(data_start, 4096).await?;
            let estimate = mp3_duration(&frames, len - data_start);
            if key.is_some() {
                warn!("license carries a key but the file is MP3; streaming it as is");
            }
            (
                Format::Mp3 {
                    data_start,
                    estimate,
                },
                Codec::plain(),
                None,
            )
        } else {
            let layout = read_layout(&mut head).await?;
            let duration = layout.sound.as_ref().and_then(sound_duration);
            (Format::Mp4, Codec::new(layout, key, len)?, duration)
        };
        let bufs = std::mem::take(&mut head.bufs);
        drop(head);

        let originals = codec.originals.clone();
        let source = Self {
            fetcher,
            options,
            len,
            format,
            codec,
            duration,
            cache_path,
            sidecar_path,
            fingerprint,
            file,
            state: Mutex::new(State {
                ranges,
                inflight: Vec::new(),
                originals,
            }),
            fetched: Notify::new(),
        };
        for (start, bytes) in bufs {
            source.ingest(start, bytes)?;
        }
        source.persist(&source.lock());
        info!(
            len,
            content_type = source.content_type(),
            cached = source.cached_fraction(),
            "stream opened"
        );
        Ok(source)
    }

    /// Total file length in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Always `false`: empty files are rejected by [`open`](Self::open).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// UTI for AVFoundation: `com.apple.m4a-audio` for MP4 (AAX/AAXC/M4A/M4B)
    /// and `public.mp3` for MP3.
    pub fn content_type(&self) -> &'static str {
        match self.format {
            Format::Mp4 => UTI_M4A,
            Format::Mp3 { .. } => UTI_MP3,
        }
    }

    /// Duration in seconds from the audio track's `mdhd`; `None` for MP3 or
    /// when the track has no usable timing.
    pub fn duration(&self) -> Option<f64> {
        self.duration
    }

    /// The cache file. Holds decrypted bytes at their real offsets; complete
    /// and playable once [`is_complete`](Self::is_complete).
    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }

    /// Replace the CDN URL, e.g. after re-licensing because the old one
    /// expired ([`Error::UrlExpired`]).
    pub fn set_url(&self, url: Secret) {
        *self
            .fetcher
            .url
            .write()
            .unwrap_or_else(PoisonError::into_inner) = url;
    }

    /// Fraction of the file already cached, `0.0..=1.0`.
    pub fn cached_fraction(&self) -> f32 {
        (self.lock().ranges.total() as f64 / self.len as f64).min(1.0) as f32
    }

    /// Whether every byte is cached (the cache file is then a complete,
    /// playable file).
    pub fn is_complete(&self) -> bool {
        self.lock().ranges.covers(0, self.len)
    }

    /// Decrypted bytes `[offset, offset + len)`, clamped to the end of the
    /// file (empty past the end). Fetches, decrypts and caches whatever is
    /// missing; correct for any offset and length.
    ///
    /// The whole range is fetched before anything is returned, so a resource
    /// loader answering "everything to the end" should read in pieces of a
    /// few MB and respond to each as it arrives.
    pub async fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        if offset >= self.len || len == 0 {
            return Ok(Vec::new());
        }
        let end = offset.saturating_add(len as u64).min(self.len);
        self.ensure(offset, end).await?;
        let mut buf = vec![0u8; usize_of(end - offset)?];
        read_exact_at(&self.file, &mut buf, offset)?;
        Ok(buf)
    }

    /// Make sure the audio for book time `[start_s, end_s)` (plus
    /// [`StreamOptions::prefetch_margin`] either side) is in the cache file,
    /// so another reader (e.g. `AVAudioFile` on [`cache_path`](Self::cache_path))
    /// can decode it. Uses the audio track's sample timing; for MP3, or MP4
    /// without timing, maps time proportionally with a generous margin (or
    /// fetches everything when the duration is unknown). `progress` receives
    /// `0.0..=1.0`.
    pub async fn prefetch_time(
        &self,
        start_s: f64,
        end_s: f64,
        progress: &(dyn Fn(f32) + Send + Sync),
    ) -> Result<()> {
        if !(start_s.is_finite() && end_s.is_finite() && start_s <= end_s) {
            return Err(Error::InvalidArgument(format!(
                "bad prefetch window {start_s}..{end_s}"
            )));
        }
        let spans = self.time_spans(start_s, end_s);
        let total: u64 = {
            let st = self.lock();
            spans.iter().map(|&(a, b)| st.ranges.missing(a, b)).sum()
        };
        progress(0.0);
        // Small chunks, several at a time, in order: a slow connection still
        // finishes each request in time, a fast one stays busy, and the
        // start of the window (what read along needs first) arrives first.
        // Chunks end on sample boundaries; otherwise neighbours' requests,
        // widened to whole samples, overlap and wait on each other.
        let chunk = self.options.prefetch_chunk.max(1);
        let mut chunks = Vec::new();
        for (a, b) in spans {
            let mut pos = a;
            while pos < b {
                let end = self
                    .codec
                    .align_end(pos.saturating_add(chunk))
                    .clamp(pos + 1, b);
                chunks.push((pos, end));
                pos = end;
            }
        }
        let mut fetches = futures_util::stream::iter(chunks)
            .map(|(a, b)| self.ensure(a, b))
            .buffered(self.options.prefetch_parallel.max(1));
        let mut done = 0u64;
        while let Some(fetched) = fetches.next().await {
            done += fetched?;
            if total > 0 {
                progress((done as f64 / total as f64).min(1.0) as f32);
            }
        }
        progress(1.0);
        Ok(())
    }

    // ----- internals -------------------------------------------------------

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Fetch whatever of `[start, end)` is not cached, one request at a time
    /// so concurrent readers interleave. Returns the bytes fetched.
    async fn ensure(&self, start: u64, end: u64) -> Result<u64> {
        enum Step<'a> {
            Wait(tokio::sync::futures::Notified<'a>),
            Fetch { piece: (u64, u64), need: u64 },
        }
        let mut fetched = 0;
        loop {
            let step = {
                let mut st = self.lock();
                let Some((gap_start, gap_end)) = st.ranges.first_gap(start, end) else {
                    return Ok(fetched);
                };
                let piece = self.plan(&st, gap_start, gap_end);
                if st.inflight.iter().any(|r| r.0 < piece.1 && piece.0 < r.1) {
                    // Created under the lock, so a fetch finishing after we
                    // looked cannot be missed.
                    Step::Wait(self.fetched.notified())
                } else {
                    st.inflight.push(piece);
                    Step::Fetch {
                        piece,
                        need: gap_start,
                    }
                }
            };
            match step {
                Step::Wait(notified) => notified.await,
                Step::Fetch { piece, need } => {
                    let _guard = Inflight {
                        source: self,
                        range: piece,
                    };
                    let (bytes, _) = self
                        .fetcher
                        .get(
                            piece.0,
                            piece.1,
                            Some(self.len),
                            &self.options,
                            self.options.retry.max_attempts,
                        )
                        .await?;
                    let (lo, hi) = self.ingest(piece.0, bytes)?;
                    if !(lo <= need && need < hi) {
                        // Only possible with a corrupt sample table; bail out
                        // instead of fetching the same bytes forever.
                        return Err(Error::InvalidMedia(format!(
                            "could not decode the samples around offset {need}"
                        )));
                    }
                    fetched += piece.1 - piece.0;
                }
            }
        }
    }

    /// The request for a gap starting at `gap_start`: whole samples, at
    /// least `min_fetch` (without running into cached or in-flight bytes)
    /// and at most `max_fetch`.
    fn plan(&self, st: &State, gap_start: u64, gap_end: u64) -> (u64, u64) {
        let o = &self.options;
        let next_cached = st.ranges.next_start(gap_start).unwrap_or(self.len);
        let next_inflight = st
            .inflight
            .iter()
            .map(|r| r.0)
            .filter(|&s| s > gap_start)
            .min()
            .unwrap_or(self.len);
        let limit = next_cached.min(next_inflight).min(self.len);
        let target = gap_end
            .max(gap_start.saturating_add(o.min_fetch))
            .min(gap_start.saturating_add(o.max_fetch))
            .min(limit)
            .max(gap_start + 1);
        let start = self.codec.align_start(gap_start);
        let end = self.codec.align_end(target).min(self.len);
        (start, end)
    }

    /// Decrypt and patch raw file bytes starting at `start`, write the part
    /// that is now plaintext to the cache and record it.
    fn ingest(&self, start: u64, mut bytes: Vec<u8>) -> Result<(u64, u64)> {
        let (lo, hi) = self.codec.decode(start, &mut bytes);
        if lo < hi {
            let from = usize_of(lo - start)?;
            let to = usize_of(hi - start)?;
            write_all_at(&self.file, &bytes[from..to], lo)?;
            let mut st = self.lock();
            st.ranges.insert(lo, hi);
            self.persist(&st);
        }
        Ok((lo, hi))
    }

    /// Write the sidecar atomically. Failures only cost the cache on the next
    /// open, so they are logged rather than returned.
    fn persist(&self, st: &State) {
        let sidecar = Sidecar {
            version: SIDECAR_VERSION,
            len: self.len,
            key: self.fingerprint.clone(),
            ranges: st.ranges.0.clone(),
            originals: st.originals.clone(),
        };
        let tmp = sibling(&self.sidecar_path, ".tmp");
        let result = serde_json::to_vec(&sidecar)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(&tmp, json))
            .and_then(|()| std::fs::rename(&tmp, &self.sidecar_path));
        if let Err(e) = result {
            warn!(error = %e, "could not save the stream cache index");
        }
    }

    /// Byte spans holding the audio for `[start_s, end_s)` plus margins.
    fn time_spans(&self, start_s: f64, end_s: f64) -> Vec<(u64, u64)> {
        let margin = self.options.prefetch_margin.max(0.0);
        let (data_start, estimate) = match self.format {
            Format::Mp4 => {
                let layout = &self.codec.layout;
                let chunks = layout.sound_chunks();
                if let Some(sound) = layout.sound.as_ref().filter(|s| s.timescale > 0)
                    && !chunks.is_empty()
                {
                    let ts = f64::from(sound.timescale);
                    let t0 = ((start_s - margin).max(0.0) * ts).floor() as u64;
                    let t1 = ((end_s + margin).max(0.0) * ts).ceil() as u64;
                    let s0 = sample_at_time(&sound.stts, t0);
                    let s1 = sample_at_time(&sound.stts, t1).saturating_add(1);
                    return sample_spans(chunks, s0, s1);
                }
                (0, None)
            }
            Format::Mp3 {
                data_start,
                estimate,
            } => (data_start, estimate),
        };
        // No timing: place the window proportionally, generously.
        let Some(duration) = estimate.filter(|d| *d > 0.0) else {
            return vec![(0, self.len)];
        };
        let slack = margin.max(10.0) + 0.02 * duration;
        let data_len = (self.len - data_start) as f64;
        let at = |t: f64| (t / duration).clamp(0.0, 1.0) * data_len;
        let a = data_start + at(start_s - slack).floor() as u64;
        let b = (data_start + at(end_s + slack).ceil() as u64).min(self.len);
        vec![(a, b)]
    }
}

/// Removes an in-flight range when its fetch ends, however it ends
/// (success, error, or the reading future being dropped).
struct Inflight<'a> {
    source: &'a StreamSource,
    range: (u64, u64),
}

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        {
            let mut st = self.source.lock();
            if let Some(i) = st.inflight.iter().position(|r| *r == self.range) {
                st.inflight.swap_remove(i);
            }
        }
        self.source.fetched.notify_waiters();
    }
}

// ----- decryption -----------------------------------------------------------

/// An encrypted chunk, for lookups by file offset.
#[derive(Debug, Clone, Copy)]
struct EncChunk {
    offset: u64,
    end: u64,
    track: usize,
    chunk: usize,
}

/// Everything needed to turn raw file bytes into playable bytes.
#[derive(Debug)]
struct Codec {
    layout: Layout,
    /// Encrypted chunks of all tracks, sorted by offset, non-overlapping.
    enc: Vec<EncChunk>,
    /// Key and IV per encrypted track.
    keys: Vec<([u8; 16], [u8; 16])>,
    /// Same-size patches that make the file playable.
    patches: Vec<Patch>,
    /// The bytes those patches replace.
    originals: Vec<Patch>,
}

impl Codec {
    /// Pass-through (DRM-free MP3).
    fn plain() -> Self {
        Self {
            layout: Layout {
                ftyp: None,
                ftyp_payload: Vec::new(),
                tracks: Vec::new(),
                sound: None,
            },
            enc: Vec::new(),
            keys: Vec::new(),
            patches: Vec::new(),
            originals: Vec::new(),
        }
    }

    fn new(layout: Layout, key: Option<&DecryptionKey>, file_len: u64) -> Result<Self> {
        let keys = match (key, layout.tracks.is_empty()) {
            (_, true) => {
                if key.is_some() {
                    warn!(
                        "license carries a key but the file is not encrypted; streaming it as is"
                    );
                }
                Vec::new()
            }
            (Some(key), false) => layout
                .tracks
                .iter()
                .map(|t| track_key(t, key))
                .collect::<Result<_>>()?,
            (None, false) => {
                return Err(Error::InvalidMedia(
                    "the file is encrypted but the license carries no key".into(),
                ));
            }
        };
        let mut enc = Vec::new();
        for (track, t) in layout.tracks.iter().enumerate() {
            for (chunk, c) in t.chunks.iter().enumerate() {
                if !c.sample_sizes.is_empty() {
                    enc.push(EncChunk {
                        offset: c.offset,
                        end: c.offset + c.len(),
                        track,
                        chunk,
                    });
                }
            }
        }
        enc.sort_unstable_by_key(|c| c.offset);
        if enc.windows(2).any(|w| w[0].end > w[1].offset)
            || enc.last().is_some_and(|c| c.end > file_len)
        {
            return Err(Error::InvalidMedia(
                "encrypted samples overlap or run past the end of the file".into(),
            ));
        }
        let (patches, originals) = patches(&layout);
        Ok(Self {
            layout,
            enc,
            keys,
            patches,
            originals,
        })
    }

    fn sizes(&self, c: &EncChunk) -> &[u32] {
        &self.layout.tracks[c.track].chunks[c.chunk].sample_sizes
    }

    /// The encrypted sample `[start, end)` containing `pos`.
    fn sample_at(&self, pos: u64) -> Option<(u64, u64)> {
        let c = self.enc.get(self.enc.partition_point(|c| c.end <= pos))?;
        if pos < c.offset {
            return None;
        }
        let mut start = c.offset;
        for size in self.sizes(c) {
            let end = start + u64::from(*size);
            if pos < end {
                return Some((start, end));
            }
            start = end;
        }
        None
    }

    /// Move `pos` back to the start of the sample it falls in.
    fn align_start(&self, pos: u64) -> u64 {
        self.sample_at(pos).map_or(pos, |s| s.0)
    }

    /// Move an exclusive end forward to the end of the sample it cuts.
    fn align_end(&self, pos: u64) -> u64 {
        match self.sample_at(pos) {
            Some((s, e)) if s < pos => e,
            _ => pos,
        }
    }

    /// Decrypt every encrypted sample lying wholly inside `buf` (file bytes
    /// from `start`) and apply the patches. Returns the absolute sub-range
    /// that is now final: samples cut by either end are excluded.
    fn decode(&self, start: u64, buf: &mut [u8]) -> (u64, u64) {
        let end = start + buf.len() as u64;
        let (mut lo, mut hi) = (start, end);
        let first = self.enc.partition_point(|c| c.end <= start);
        for c in self.enc[first..].iter().take_while(|c| c.offset < end) {
            let (key, iv) = &self.keys[c.track];
            let mut s = c.offset;
            for size in self.sizes(c) {
                let e = s + u64::from(*size);
                if s >= end {
                    break;
                }
                if e > start {
                    if s >= start && e <= end {
                        let range = (s - start) as usize..(e - start) as usize;
                        aes128_cbc_decrypt_blocks(key, iv, &mut buf[range]);
                    } else {
                        if s < start {
                            lo = lo.max(e);
                        }
                        if e > end {
                            hi = hi.min(s);
                        }
                    }
                }
                s = e;
            }
        }
        apply_patches(start, buf, &self.patches);
        (lo, hi.max(lo))
    }
}

/// The patches [`crate::drm`]'s converter applies, and the bytes they
/// replace.
fn patches(layout: &Layout) -> (Vec<Patch>, Vec<Patch>) {
    let mut new = Vec::new();
    let mut old = Vec::new();
    if let Some(ftyp) = layout.ftyp {
        for (i, brand) in layout.ftyp_payload.chunks_exact(4).enumerate() {
            // Index 1 is the minor version, not a brand.
            if i != 1 && brand.starts_with(b"aax") {
                let at = ftyp.payload_start() + 4 * i as u64;
                let mut b = [0u8; 4];
                b.copy_from_slice(brand);
                new.push((at, *b"M4B "));
                old.push((at, b));
            }
        }
    }
    for t in &layout.tracks {
        new.push((t.sample_entry.start + 4, *b"mp4a"));
        old.push((t.sample_entry.start + 4, t.sample_entry.kind));
        for child in &t.entry_children {
            if &child.kind == b"adrm" || &child.kind == b"aabd" {
                new.push((child.start + 4, *b"free"));
                old.push((child.start + 4, child.kind));
            }
        }
    }
    (new, old)
}

/// Write the parts of `patches` that fall inside `buf` (file bytes from
/// `start`).
fn apply_patches(start: u64, buf: &mut [u8], patches: &[Patch]) {
    let end = start + buf.len() as u64;
    for (at, bytes) in patches {
        for (i, b) in bytes.iter().enumerate() {
            let p = at + i as u64;
            if p >= start && p < end {
                buf[(p - start) as usize] = *b;
            }
        }
    }
}

// ----- timing ---------------------------------------------------------------

fn sound_duration(sound: &mp4::SoundTrack) -> Option<f64> {
    if sound.timescale == 0 {
        return None;
    }
    let mut units = sound.duration;
    if units == 0 || units == u64::from(u32::MAX) || units == u64::MAX {
        units = sound
            .stts
            .iter()
            .map(|&(n, d)| u64::from(n) * u64::from(d))
            .sum();
    }
    (units > 0).then(|| units as f64 / f64::from(sound.timescale))
}

/// Index of the sample playing at media time `t` (timescale units); the
/// sample count when `t` is past the end.
fn sample_at_time(stts: &[(u32, u32)], t: u64) -> u64 {
    let (mut acc_t, mut acc_s) = (0u64, 0u64);
    for &(count, delta) in stts {
        let span = u64::from(count) * u64::from(delta);
        if delta > 0 && t < acc_t + span {
            return acc_s + (t - acc_t) / u64::from(delta);
        }
        acc_t += span;
        acc_s += u64::from(count);
    }
    acc_s
}

/// Byte spans of samples `[s0, s1)` of a track, merging nearby spans.
fn sample_spans(chunks: &[mp4::Chunk], s0: u64, s1: u64) -> Vec<(u64, u64)> {
    let mut spans: Vec<(u64, u64)> = Vec::new();
    let mut n = 0u64;
    for c in chunks {
        let k = c.sample_sizes.len() as u64;
        if n >= s1 {
            break;
        }
        if n + k > s0 {
            let from = s0.saturating_sub(n) as usize;
            let to = (s1 - n).min(k) as usize;
            let sum = |s: &[u32]| s.iter().map(|x| u64::from(*x)).sum::<u64>();
            let a = c.offset + sum(&c.sample_sizes[..from]);
            let b = a + sum(&c.sample_sizes[from..to]);
            match spans.last_mut() {
                Some(last) if a >= last.1 && a - last.1 <= MERGE_GAP => last.1 = last.1.max(b),
                _ => spans.push((a, b)),
            }
        }
        n += k;
    }
    spans
}

// ----- MP3 ------------------------------------------------------------------

fn is_mp3(magic: &[u8]) -> bool {
    magic.starts_with(b"ID3") || (magic.len() >= 2 && magic[0] == 0xFF && magic[1] & 0xE0 == 0xE0)
}

/// Length of a leading ID3v2 tag (0 when there is none).
fn id3_len(magic: &[u8]) -> u64 {
    if !magic.starts_with(b"ID3") || magic.len() < 10 {
        return 0;
    }
    let size = magic[6..10]
        .iter()
        .fold(0u64, |acc, b| (acc << 7) | u64::from(b & 0x7F));
    let footer = if magic[5] & 0x10 != 0 { 10 } else { 0 };
    10 + size + footer
}

/// Estimate an MP3's duration from its first Layer III frame: the Xing/Info
/// frame count when present, else the bitrate (exact for CBR).
fn mp3_duration(buf: &[u8], data_len: u64) -> Option<f64> {
    const BR1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const BR2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let i = buf
        .windows(2)
        .position(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0)?;
    let h = buf.get(i..i + 4)?;
    let version = (h[1] >> 3) & 3; // 3 = MPEG-1, 2 = MPEG-2, 0 = MPEG-2.5
    let layer = (h[1] >> 1) & 3; // 1 = Layer III
    if layer != 1 || version == 1 {
        return None;
    }
    let mpeg1 = version == 3;
    let kbps = *if mpeg1 { &BR1 } else { &BR2 }.get(usize::from(h[2] >> 4))?;
    let base_rate = *[44100u32, 48000, 32000].get(usize::from((h[2] >> 2) & 3))?;
    let rate = base_rate
        >> if mpeg1 {
            0
        } else if version == 2 {
            1
        } else {
            2
        };
    let mono = h[3] >> 6 == 3;
    let side = match (mpeg1, mono) {
        (true, false) => 32,
        (true, true) | (false, false) => 17,
        (false, true) => 9,
    };
    let per_frame = if mpeg1 { 1152.0 } else { 576.0 };
    if let Some(x) = buf.get(i + 4 + side..i + 4 + side + 12)
        && (&x[..4] == b"Xing" || &x[..4] == b"Info")
        && x[7] & 1 != 0
    {
        let frames = u32::from_be_bytes([x[8], x[9], x[10], x[11]]);
        if frames > 0 {
            return Some(f64::from(frames) * per_frame / f64::from(rate));
        }
    }
    (kbps > 0).then(|| data_len as f64 * 8.0 / (f64::from(kbps) * 1000.0))
}

// ----- opening --------------------------------------------------------------

/// Reads file bytes while opening: from earlier responses, then the cache
/// (with patches undone, so the parser sees the original boxes), then the
/// network. Network responses are kept to seed the cache.
struct HeadReader<'a> {
    fetcher: &'a Fetcher,
    options: &'a StreamOptions,
    len: u64,
    bufs: Vec<(u64, Vec<u8>)>,
    file: &'a File,
    ranges: &'a RangeSet,
    originals: &'a [Patch],
    /// Why the network is unavailable, when opening offline.
    offline: Option<Error>,
}

impl HeadReader<'_> {
    async fn read(&mut self, start: u64, len: u64) -> Result<Vec<u8>> {
        let start = start.min(self.len);
        let end = start.saturating_add(len).min(self.len);
        for (s, b) in &self.bufs {
            if *s <= start && s + b.len() as u64 >= end {
                return Ok(b[usize_of(start - s)?..usize_of(end - s)?].to_vec());
            }
        }
        if self.ranges.covers(start, end) {
            let mut buf = vec![0u8; usize_of(end - start)?];
            read_exact_at(self.file, &mut buf, start)?;
            apply_patches(start, &mut buf, self.originals);
            return Ok(buf);
        }
        if let Some(e) = self.offline.take() {
            return Err(e);
        }
        if start == end {
            return Ok(Vec::new());
        }
        // Small reads (box headers) fetch a probe-sized window; box payloads
        // fetch 16 bytes more, which usually holds the next box header.
        let fetch_end = end
            .max(start.saturating_add(self.options.probe_len))
            .saturating_add(16)
            .min(self.len);
        let (bytes, _) = self
            .fetcher
            .get(
                start,
                fetch_end,
                Some(self.len),
                self.options,
                self.options.retry.max_attempts,
            )
            .await?;
        let out = bytes[..usize_of(end - start)?].to_vec();
        self.bufs.push((start, bytes));
        Ok(out)
    }
}

/// Walk the top-level boxes and parse `moov`, wherever it is.
async fn read_layout(head: &mut HeadReader<'_>) -> Result<Layout> {
    let len = head.len;
    let mut top = Vec::new();
    let mut pos = 0u64;
    while pos + 8 <= len {
        if top.len() >= MAX_TOP_BOXES {
            return Err(Error::InvalidMedia("too many top-level boxes".into()));
        }
        let bytes = head.read(pos, 16).await?;
        let h = mp4::top_header(&bytes, pos, len)?;
        pos = h.end();
        top.push(h);
    }
    let moov = mp4::find_moov(&top)?;
    let moov_buf = head.read(moov.start, moov.size).await?;
    let ftyp_payload = match mp4::find_ftyp(&top) {
        Some(f) if f.size > MAX_FTYP => {
            return Err(Error::InvalidMedia("ftyp box is implausibly large".into()));
        }
        Some(f) => head.read(f.payload_start(), f.size - f.header_len).await?,
        None => Vec::new(),
    };
    mp4::parse_moov(&top, &moov_buf, ftyp_payload)
}

// ----- HTTP -----------------------------------------------------------------

struct Fetcher {
    client: reqwest::Client,
    /// Pre-signed URL; never logged.
    url: RwLock<Secret>,
}

impl Fetcher {
    /// GET `[start, end)` with retries. Returns the bytes (fewer when the
    /// file ends before `end`) and the file's total length. `total` is the
    /// length already known, if any.
    async fn get(
        &self,
        start: u64,
        end: u64,
        total: Option<u64>,
        options: &StreamOptions,
        attempts: u32,
    ) -> Result<(Vec<u8>, u64)> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self
                .get_once(start, end, total, options.request_timeout)
                .await
            {
                Ok(v) => return Ok(v),
                Err(e) if e.is_transient() && attempt < attempts => {
                    let delay = match &e {
                        Error::RateLimited {
                            retry_after: Some(d),
                        } => *d,
                        _ => options.retry.backoff(attempt),
                    }
                    .min(options.retry.max_delay);
                    warn!(attempt, ?delay, error = %e, "stream request failed; retrying");
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn get_once(
        &self,
        start: u64,
        end: u64,
        total: Option<u64>,
        timeout: Duration,
    ) -> Result<(Vec<u8>, u64)> {
        let url = self
            .url
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let resp = self
            .client
            .get(url.expose())
            .header(RANGE, format!("bytes={start}-{}", end.saturating_sub(1)))
            .header(USER_AGENT, DOWNLOAD_USER_AGENT)
            .timeout(timeout)
            .send()
            .await?;
        let status = resp.status();
        match status {
            StatusCode::PARTIAL_CONTENT => {
                let (got_start, got_total) = parse_content_range(resp.headers())
                    .ok_or_else(|| Error::InvalidMedia("206 reply without Content-Range".into()))?;
                let got_total = got_total.ok_or_else(|| {
                    Error::InvalidMedia("the CDN did not report the file length".into())
                })?;
                if got_start != start {
                    return Err(Error::InvalidMedia(format!(
                        "asked for bytes from {start}, got bytes from {got_start}"
                    )));
                }
                if total.is_some_and(|t| t != got_total) {
                    return Err(Error::InvalidMedia(
                        "the remote file changed size while streaming".into(),
                    ));
                }
                let want = end.min(got_total).saturating_sub(start);
                let mut body = Vec::from(resp.bytes().await?);
                if (body.len() as u64) < want {
                    return Err(Error::Incomplete {
                        downloaded: body.len() as u64,
                        expected: want,
                    });
                }
                body.truncate(usize_of(want)?);
                Ok((body, got_total))
            }
            StatusCode::OK => {
                // The Range header was ignored: only usable when the whole
                // file was asked for anyway.
                let whole = start == 0
                    && match total {
                        Some(t) => end >= t,
                        None => resp.content_length().is_some_and(|l| l <= end),
                    };
                if !whole {
                    return Err(Error::InvalidMedia(
                        "the CDN ignored the Range header".into(),
                    ));
                }
                let body = Vec::from(resp.bytes().await?);
                let len = body.len() as u64;
                if total.is_some_and(|t| t != len) {
                    return Err(Error::InvalidMedia(
                        "the remote file changed size while streaming".into(),
                    ));
                }
                Ok((body, len))
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::GONE => {
                Err(Error::UrlExpired {
                    status: status.as_u16(),
                })
            }
            s => {
                let retry_after = resp
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse().ok())
                    .map(Duration::from_secs);
                Err(map_status(
                    s,
                    retry_after,
                    format!("stream request failed: {s}"),
                ))
            }
        }
    }
}

// ----- cache index ----------------------------------------------------------

/// Sorted, disjoint, non-adjacent half-open byte ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RangeSet(Vec<(u64, u64)>);

impl RangeSet {
    fn from_vec(ranges: Vec<(u64, u64)>) -> Self {
        let mut set = Self::default();
        for (a, b) in ranges {
            set.insert(a, b);
        }
        set
    }

    fn insert(&mut self, a: u64, b: u64) {
        if a >= b {
            return;
        }
        let i = self.0.partition_point(|r| r.1 < a);
        let (mut lo, mut hi) = (a, b);
        let mut j = i;
        while let Some(r) = self.0.get(j).filter(|r| r.0 <= hi) {
            lo = lo.min(r.0);
            hi = hi.max(r.1);
            j += 1;
        }
        self.0.splice(i..j, [(lo, hi)]);
    }

    fn covers(&self, a: u64, b: u64) -> bool {
        a >= b
            || self
                .0
                .get(self.0.partition_point(|r| r.1 <= a))
                .is_some_and(|r| r.0 <= a && b <= r.1)
    }

    /// The first part of `[a, b)` that is not covered.
    fn first_gap(&self, a: u64, b: u64) -> Option<(u64, u64)> {
        let i = self.0.partition_point(|r| r.1 <= a);
        let (start, next) = match self.0.get(i) {
            Some(r) if r.0 <= a => (r.1, self.0.get(i + 1)),
            other => (a, other),
        };
        (start < b).then(|| (start, next.map_or(b, |r| r.0.min(b))))
    }

    /// Start of the first range beginning after `pos`.
    fn next_start(&self, pos: u64) -> Option<u64> {
        self.0
            .get(self.0.partition_point(|r| r.0 <= pos))
            .map(|r| r.0)
    }

    /// Bytes of `[a, b)` not covered.
    fn missing(&self, a: u64, b: u64) -> u64 {
        if a >= b {
            return 0;
        }
        let covered: u64 = self
            .0
            .iter()
            .map(|r| r.1.min(b).saturating_sub(r.0.max(a)))
            .sum();
        (b - a) - covered
    }

    fn total(&self) -> u64 {
        self.0.iter().map(|r| r.1 - r.0).sum()
    }
}

/// The `.ranges` sidecar.
#[derive(Debug, Serialize, Deserialize)]
struct Sidecar {
    version: u32,
    /// Remote file length.
    len: u64,
    /// Truncated SHA-256 of the key material (or of "none").
    key: String,
    /// Cached byte ranges, half-open.
    ranges: Vec<(u64, u64)>,
    /// Original bytes under the playability patches.
    #[serde(default)]
    originals: Vec<Patch>,
}

impl Sidecar {
    fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes)
            .map_err(|e| warn!(error = %e, "ignoring an unreadable stream cache index"))
            .ok()
    }
}

/// Fingerprint of the key material; changes whenever the key does.
fn key_fingerprint(key: Option<&DecryptionKey>) -> String {
    let mut h = Sha256::new();
    h.update(b"unabridged-stream-cache\0");
    match key {
        None => h.update(b"none"),
        Some(DecryptionKey::Aaxc { key, iv }) => {
            h.update(b"aaxc");
            h.update(key);
            h.update(iv);
        }
        Some(DecryptionKey::Aax(ab)) => {
            h.update(b"aax");
            h.update(ab.0);
        }
    }
    hex::encode(&h.finalize()[..16])
}

/// `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn usize_of(n: u64) -> Result<usize> {
    usize::try_from(n).map_err(|_| Error::InvalidArgument("range too large".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rand::{Rng, SeedableRng};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use crate::drm::ActivationBytes;
    use crate::drm::convert::decrypt_file;
    use crate::drm::convert::tests::make_encrypted;
    use crate::models::Asin;

    const KEY: [u8; 16] = [0x11; 16];
    const IV: [u8; 16] = [0x22; 16];

    #[test]
    fn positional_read_handles_short_reads_and_eof() {
        let source = b"abcdefg";
        let mut offsets = Vec::new();
        let mut buf = [0; 6];
        read_exact_at_with(&mut buf, 1, |buf, offset| {
            offsets.push(offset);
            if offsets.len() == 1 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let n = buf.len().min(2);
            buf[..n].copy_from_slice(&source[offset as usize..offset as usize + n]);
            Ok(n)
        })
        .unwrap();
        assert_eq!(&buf, b"bcdefg");
        assert_eq!(offsets, [1, 1, 3, 5]);

        let error = read_exact_at_with(&mut buf, 5, |buf, offset| {
            let remaining = &source[offset as usize..];
            let n = remaining.len().min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            Ok(n)
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        read_exact_at_with(&mut [], 99, |_, _| panic!("empty read")).unwrap();
        let error = read_exact_at_with(&mut buf, 0, |_, _| {
            Err(io::ErrorKind::PermissionDenied.into())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn positional_write_handles_short_writes_and_errors() {
        let mut output = [0; 8];
        let mut offsets = Vec::new();
        write_all_at_with(b"abcde", 1, |buf, offset| {
            offsets.push(offset);
            if offsets.len() == 1 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let n = buf.len().min(2);
            output[offset as usize..offset as usize + n].copy_from_slice(&buf[..n]);
            Ok(n)
        })
        .unwrap();
        assert_eq!(&output, b"\0abcde\0\0");
        assert_eq!(offsets, [1, 1, 3, 5]);
        let error = write_all_at_with(b"a", 0, |_, _| Ok(0)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        write_all_at_with(&[], 99, |_, _| panic!("empty write")).unwrap();
        let error = write_all_at_with(b"a", 0, |_, _| Err(io::ErrorKind::PermissionDenied.into()))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn positional_file_io_uses_explicit_offsets() {
        use std::io::{Seek, SeekFrom};

        let mut file = tempfile::tempfile().unwrap();
        file.seek(SeekFrom::Start(100)).unwrap();
        write_all_at(&file, b"abcdef", 0).unwrap();
        file.seek(SeekFrom::Start(100)).unwrap();
        let mut buf = [0; 3];
        read_exact_at(&file, &mut buf, 2).unwrap();
        assert_eq!(&buf, b"cde");
        let error = read_exact_at(&file, &mut buf, 4).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        read_exact_at(&file, &mut [], 100).unwrap();
        write_all_at(&file, &[], 100).unwrap();
    }

    fn sine() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sine.m4a"
        ))
        .unwrap()
    }

    /// Serves `data` honouring `Range: bytes=a-b`, counting requests.
    struct Ranged {
        data: Arc<Vec<u8>>,
        hits: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl Respond for Ranged {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            self.hits.fetch_add(1, Ordering::SeqCst);
            let len = self.data.len() as u64;
            let Some(range) = req.headers.get("range").and_then(|v| v.to_str().ok()) else {
                return ResponseTemplate::new(200).set_body_bytes(self.data.to_vec());
            };
            let (a, b) = range
                .strip_prefix("bytes=")
                .and_then(|r| r.split_once('-'))
                .unwrap();
            let a: u64 = a.parse().unwrap();
            let b: u64 = if b.is_empty() {
                len - 1
            } else {
                b.parse::<u64>().unwrap().min(len - 1)
            };
            if a >= len {
                return ResponseTemplate::new(416)
                    .insert_header("Content-Range", format!("bytes */{len}").as_str());
            }
            ResponseTemplate::new(206)
                .insert_header("Content-Range", format!("bytes {a}-{b}/{len}").as_str())
                .set_body_bytes(self.data[a as usize..=b as usize].to_vec())
                .set_delay(self.delay)
        }
    }

    struct Cdn {
        server: MockServer,
        hits: Arc<AtomicUsize>,
    }

    impl Cdn {
        async fn serving(data: Vec<u8>) -> Self {
            Self::serving_slowly(data, Duration::ZERO).await
        }

        /// Each ranged response waits `delay` before it's sent.
        async fn serving_slowly(data: Vec<u8>, delay: Duration) -> Self {
            let server = MockServer::start().await;
            let hits = Arc::new(AtomicUsize::new(0));
            Mock::given(method("GET"))
                .and(path("/book"))
                .respond_with(Ranged {
                    data: Arc::new(data),
                    hits: hits.clone(),
                    delay,
                })
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/expired"))
                .respond_with(ResponseTemplate::new(403).set_body_string("AccessDenied"))
                .mount(&server)
                .await;
            Self { server, hits }
        }

        fn url(&self) -> String {
            format!("{}/book?Signature=SECRET", self.server.uri())
        }

        fn expired(&self) -> String {
            format!("{}/expired?Signature=OLD", self.server.uri())
        }

        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    fn license(url: String, drm: ContentDrm) -> License {
        License {
            asin: Asin::new("B002V5D7RU").unwrap(),
            content_reference: None,
            chapter_info: None,
            drm,
            download_url: Secret::new(url),
        }
    }

    fn aaxc() -> ContentDrm {
        ContentDrm::Aaxc(DecryptionKey::Aaxc { key: KEY, iv: IV })
    }

    fn small() -> StreamOptions {
        StreamOptions {
            probe_len: 64,
            min_fetch: 100,
            max_fetch: 1000,
            prefetch_margin: 0.0,
            prefetch_chunk: 100,
            prefetch_parallel: 4,
            retry: RetryPolicy {
                max_attempts: 2,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
            },
            request_timeout: Duration::from_secs(10),
        }
    }

    /// An encrypted book and the converter's output for it.
    struct Book {
        encrypted: Vec<u8>,
        plain: Vec<u8>,
    }

    fn book_from(dir: &Path, encrypted: Vec<u8>, key: &DecryptionKey) -> Book {
        let input = dir.join("in.aaxc");
        let output = dir.join("out.m4b");
        std::fs::write(&input, &encrypted).unwrap();
        decrypt_file(&input, &output, key, None).unwrap();
        Book {
            encrypted,
            plain: std::fs::read(&output).unwrap(),
        }
    }

    fn aaxc_book(dir: &Path) -> Book {
        let enc = make_encrypted(dir, None, &KEY, &IV);
        book_from(dir, enc, &DecryptionKey::Aaxc { key: KEY, iv: IV })
    }

    /// Sample boundaries of the audio track.
    fn samples(bytes: &[u8], dir: &Path) -> Vec<(u64, u64)> {
        let p = dir.join("probe.mp4");
        std::fs::write(&p, bytes).unwrap();
        let layout = mp4::parse(&mut File::open(&p).unwrap()).unwrap();
        let mut out = Vec::new();
        for c in layout.sound_chunks() {
            let mut s = c.offset;
            for size in &c.sample_sizes {
                out.push((s, s + u64::from(*size)));
                s += u64::from(*size);
            }
        }
        out
    }

    async fn open(
        cdn_url: String,
        drm: ContentDrm,
        cache: &Path,
        o: StreamOptions,
    ) -> StreamSource {
        StreamSource::open_with(
            reqwest::Client::new(),
            &license(cdn_url, drm),
            cache.to_owned(),
            o,
        )
        .await
        .unwrap()
    }

    async fn read_all(src: &StreamSource, piece: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut pos = 0u64;
        loop {
            let chunk = src.read(pos, piece).await.unwrap();
            if chunk.is_empty() {
                return out;
            }
            assert!(chunk.len() <= piece);
            pos += chunk.len() as u64;
            out.extend_from_slice(&chunk);
        }
    }

    #[test]
    fn stream_source_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<StreamSource>();
    }

    #[tokio::test]
    async fn reads_in_odd_pieces_match_decrypt_file() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        for piece in [1usize, 7, 4099, 65537] {
            let cache = dir.path().join(format!("c{piece}.m4b"));
            let src = open(cdn.url(), aaxc(), &cache, small()).await;
            assert_eq!(src.len(), book.plain.len() as u64);
            assert_eq!(src.content_type(), "com.apple.m4a-audio");
            assert_eq!(read_all(&src, piece).await, book.plain, "piece {piece}");
            assert!(src.is_complete());
            assert_eq!(std::fs::read(&cache).unwrap(), book.plain);
        }

        // Random offsets on a fresh cache.
        let src = open(cdn.url(), aaxc(), &dir.path().join("rand.m4b"), small()).await;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let n = book.plain.len();
        for _ in 0..300 {
            let off = rng.gen_range(0..n + 10);
            let len = rng.gen_range(0..3000);
            let got = src.read(off as u64, len).await.unwrap();
            let want = &book.plain[off.min(n)..(off + len).min(n)];
            assert_eq!(got, want, "read({off}, {len})");
        }
    }

    #[tokio::test]
    async fn reads_starting_and_ending_mid_sample() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        let o = StreamOptions {
            min_fetch: 1,
            ..small()
        };
        let src = open(cdn.url(), aaxc(), &dir.path().join("c.m4b"), o).await;
        let samples = samples(&book.encrypted, dir.path());
        let (s, e) = samples[10];
        assert!(e - s > 40, "fixture samples are large enough");
        let before = cdn.hits();
        // Inside one sample: exactly that sample is fetched.
        let got = src.read(s + 3, 5).await.unwrap();
        assert_eq!(got, &book.plain[s as usize + 3..s as usize + 8]);
        assert_eq!(cdn.hits(), before + 1);
        assert!(src.lock().ranges.covers(s, e));
        assert!(!src.lock().ranges.covers(e, e + 1));
        // Across a sample boundary, into an uncached sample.
        let got = src.read(e - 2, 6).await.unwrap();
        assert_eq!(got, &book.plain[e as usize - 2..e as usize + 4]);
        // Spanning several samples, ragged at both ends.
        let (s2, _) = samples[20];
        let (_, e2) = samples[24];
        let got = src.read(s2 + 1, (e2 - s2 - 2) as usize).await.unwrap();
        assert_eq!(got, &book.plain[s2 as usize + 1..e2 as usize - 1]);
        // The last byte of the file.
        let last = book.plain.len() as u64 - 1;
        assert_eq!(
            src.read(last, 100).await.unwrap(),
            &book.plain[last as usize..]
        );
    }

    #[tokio::test]
    async fn reopening_reuses_the_cache_and_a_new_key_discards_it() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        let cache = dir.path().join("book.m4b");
        {
            let src = open(cdn.url(), aaxc(), &cache, small()).await;
            assert!(src.cached_fraction() < 0.5);
            read_all(&src, 4096).await;
            assert!(src.is_complete());
        }
        assert!(sibling(&cache, ".ranges").exists());

        // Same key and length: only the length probe goes to the network.
        let before = cdn.hits();
        let src = open(cdn.url(), aaxc(), &cache, small()).await;
        assert_eq!(cdn.hits(), before + 1);
        assert!(src.is_complete());
        assert_eq!(read_all(&src, 999).await, book.plain);
        assert_eq!(cdn.hits(), before + 1);
        assert_eq!(src.duration(), Some(89224.0 / 44100.0));
        drop(src);

        // Expired URL: opens from the cache alone.
        let src = open(cdn.expired(), aaxc(), &cache, small()).await;
        assert!(src.is_complete());
        assert_eq!(read_all(&src, 5000).await, book.plain);
        drop(src);

        // A different key invalidates the cache.
        let other = ContentDrm::Aaxc(DecryptionKey::Aaxc {
            key: [9; 16],
            iv: IV,
        });
        let src = open(cdn.url(), other, &cache, small()).await;
        assert!(src.cached_fraction() < 0.5);
        let before = cdn.hits();
        src.read(1000, 10).await.unwrap();
        assert!(cdn.hits() > before);
        drop(src);

        // So does a different remote length.
        let src = open(cdn.url(), aaxc(), &cache, small()).await;
        read_all(&src, 4096).await;
        drop(src);
        let mut longer = book.encrypted.clone();
        longer.extend_from_slice(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']);
        let cdn2 = Cdn::serving(longer).await;
        let src = open(cdn2.url(), aaxc(), &cache, small()).await;
        assert_eq!(src.len(), book.plain.len() as u64 + 8);
        assert!(!src.is_complete());
        assert!(src.cached_fraction() < 0.5);
    }

    #[tokio::test]
    async fn prefetch_time_caches_that_region() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        let cache = dir.path().join("c.m4b");
        let src = open(cdn.url(), aaxc(), &cache, small()).await;

        let reports = Mutex::new(Vec::new());
        src.prefetch_time(0.5, 1.0, &|p| reports.lock().unwrap().push(p))
            .await
            .unwrap();
        let reports = reports.into_inner().unwrap();
        assert_eq!(reports.first(), Some(&0.0));
        assert_eq!(reports.last(), Some(&1.0));
        assert!(reports.windows(2).all(|w| w[0] <= w[1]));

        // 0.5 s and 1.0 s at 44.1 kHz with 1024-sample frames: 21..=43.
        let samples = samples(&book.encrypted, dir.path());
        let (a, b) = (samples[21].0, samples[43].1);
        let before = cdn.hits();
        let got = src.read(a, (b - a) as usize).await.unwrap();
        assert_eq!(
            cdn.hits(),
            before,
            "prefetched bytes are served from the cache"
        );
        assert_eq!(got, &book.plain[a as usize..b as usize]);
        let on_disk = std::fs::read(&cache).unwrap();
        assert_eq!(
            &on_disk[a as usize..b as usize],
            &book.plain[a as usize..b as usize]
        );
        // ...and only roughly that region (plus the head) was fetched.
        assert!(src.cached_fraction() < 0.6, "{}", src.cached_fraction());
        assert!(!src.lock().ranges.covers(samples[80].0, samples[80].1));

        // Default margin (3 s) covers this whole 2 s book.
        let src = open(
            cdn.url(),
            aaxc(),
            &dir.path().join("m.m4b"),
            StreamOptions {
                prefetch_margin: 3.0,
                ..small()
            },
        )
        .await;
        src.prefetch_time(1.0, 1.5, &|_| {}).await.unwrap();
        let first = samples.first().unwrap().0;
        let last = samples.last().unwrap().1;
        assert!(src.lock().ranges.covers(first, last));
        assert!(src.prefetch_time(2.0, 1.0, &|_| {}).await.is_err());
    }

    #[tokio::test]
    async fn prefetch_fetches_small_chunks_in_parallel() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let delay = Duration::from_millis(100);
        let cdn = Cdn::serving_slowly(book.encrypted.clone(), delay).await;
        let src = open(cdn.url(), aaxc(), &dir.path().join("p.m4b"), small()).await;
        let before = cdn.hits();
        let started = std::time::Instant::now();
        src.prefetch_time(0.0, 2.0, &|_| {}).await.unwrap();
        let elapsed = started.elapsed();
        let requests = cdn.hits() - before;
        assert!(requests >= 8, "small chunks: {requests} requests");
        // One at a time would take requests × delay.
        assert!(
            elapsed < delay * requests as u32 / 2,
            "{requests} requests took {elapsed:?}"
        );
        // Everything asked for is cached, in order, and decrypts right.
        for (first, last) in src.time_spans(0.0, 2.0) {
            assert!(src.lock().ranges.covers(first, last));
            assert_eq!(
                src.read(first, (last - first) as usize).await.unwrap(),
                &book.plain[first as usize..last as usize]
            );
        }
    }

    #[tokio::test]
    async fn drm_free_files_stream_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let plain = sine();
        let cdn = Cdn::serving(plain.clone()).await;
        let src = open(
            cdn.url(),
            ContentDrm::None,
            &dir.path().join("p.m4a"),
            small(),
        )
        .await;
        assert_eq!(src.content_type(), "com.apple.m4a-audio");
        assert_eq!(src.duration(), Some(89224.0 / 44100.0));
        assert_eq!(read_all(&src, 333).await, plain);
        // Timing works for plain mp4a tracks too.
        let src = open(
            cdn.url(),
            ContentDrm::None,
            &dir.path().join("q.m4a"),
            small(),
        )
        .await;
        src.prefetch_time(0.5, 1.0, &|_| {}).await.unwrap();
        let samples = samples(&plain, dir.path());
        assert!(src.lock().ranges.covers(samples[21].0, samples[43].1));
        assert!(!src.is_complete());
    }

    /// 128 kbps 44.1 kHz MPEG-1 Layer III frames after an empty ID3v2 tag.
    fn fake_mp3(frames: usize) -> Vec<u8> {
        let mut data = b"ID3\x04\x00\x00\x00\x00\x00\x00".to_vec();
        for i in 0..frames {
            let mut frame = vec![(i % 251) as u8; 417];
            frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            data.extend_from_slice(&frame);
        }
        data
    }

    #[tokio::test]
    async fn mp3_is_sniffed_and_streams_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = fake_mp3(2000); // ~52 s
        let cdn = Cdn::serving(mp3.clone()).await;
        let o = StreamOptions {
            max_fetch: 64 * 1024,
            min_fetch: 4096,
            ..small()
        };
        let src = open(cdn.url(), ContentDrm::None, &dir.path().join("a.mp3"), o).await;
        assert_eq!(src.content_type(), "public.mp3");
        assert_eq!(src.duration(), None);
        let Format::Mp3 { estimate, .. } = src.format else {
            panic!("not MP3")
        };
        let est = estimate.unwrap();
        assert!(
            (est - 2000.0 * 417.0 * 8.0 / 128_000.0).abs() < 0.01,
            "{est}"
        );
        src.prefetch_time(40.0, 41.0, &|_| {}).await.unwrap();
        assert!(!src.is_complete());
        let at = |t: f64| (10.0 + t / est * (mp3.len() - 10) as f64) as u64;
        assert!(src.lock().ranges.covers(at(39.0), at(42.0)));
        assert_eq!(read_all(&src, 10_000).await, mp3);
    }

    #[tokio::test]
    async fn expired_url_is_reported_and_set_url_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted.clone()).await;

        let err = StreamSource::open_with(
            reqwest::Client::new(),
            &license(cdn.expired(), aaxc()),
            dir.path().join("x.m4b"),
            small(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::UrlExpired { status: 403 }), "{err:?}");
        assert!(!format!("{err} {err:?}").contains("Signature"));

        let src = open(cdn.url(), aaxc(), &dir.path().join("c.m4b"), small()).await;
        src.set_url(Secret::new(cdn.expired()));
        let err = src.read(4000, 100).await.unwrap_err();
        assert!(matches!(err, Error::UrlExpired { status: 403 }), "{err:?}");
        assert!(src.lock().inflight.is_empty());
        src.set_url(Secret::new(cdn.url()));
        assert_eq!(src.read(4000, 100).await.unwrap(), &book.plain[4000..4100]);
        assert_eq!(read_all(&src, 2048).await, book.plain);
    }

    #[tokio::test]
    async fn duration_matches_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted).await;
        let src = open(cdn.url(), aaxc(), &dir.path().join("c.m4b"), small()).await;
        // mdhd of sine.m4a: 89224 units at 44100 Hz.
        let d = src.duration().unwrap();
        assert!((d - 2.023_219_954).abs() < 1e-6, "{d}");
    }

    /// Move `moov` in front of `mdat`, shifting the chunk offsets.
    fn moov_first(bytes: &[u8]) -> Vec<u8> {
        let moov_at = bytes.windows(4).position(|w| w == b"moov").unwrap() - 4;
        let moov_size = u32::from_be_bytes(bytes[moov_at..moov_at + 4].try_into().unwrap());
        assert_eq!(
            moov_at + moov_size as usize,
            bytes.len(),
            "fixture ends with moov"
        );
        let mut moov = bytes[moov_at..].to_vec();
        let stco = moov.windows(4).position(|w| w == b"stco").unwrap() - 4;
        let n = u32::from_be_bytes(moov[stco + 12..stco + 16].try_into().unwrap()) as usize;
        for i in 0..n {
            let at = stco + 16 + 4 * i;
            let v = u32::from_be_bytes(moov[at..at + 4].try_into().unwrap());
            moov[at..at + 4].copy_from_slice(&(v + moov_size).to_be_bytes());
        }
        let ftyp_len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        let mut out = bytes[..ftyp_len].to_vec();
        out.extend_from_slice(&moov);
        out.extend_from_slice(&bytes[ftyp_len..moov_at]);
        out
    }

    #[tokio::test]
    async fn moov_at_the_start_works() {
        let dir = tempfile::tempdir().unwrap();
        let enc = moov_first(&make_encrypted(dir.path(), None, &KEY, &IV));
        let book = book_from(dir.path(), enc, &DecryptionKey::Aaxc { key: KEY, iv: IV });
        assert_eq!(&book.plain[4..8], b"ftyp");
        assert_eq!(&book.plain[32..36], b"moov");
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        let src = open(cdn.url(), aaxc(), &dir.path().join("c.m4b"), small()).await;
        assert_eq!(src.duration(), Some(89224.0 / 44100.0));
        assert_eq!(read_all(&src, 777).await, book.plain);
    }

    #[tokio::test]
    async fn aax_keys_from_activation_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let payload = hex::decode(crate::drm::aax::tests::ADRM_PAYLOAD).unwrap();
        let key = crate::crypto::hex16(crate::drm::convert::tests::AAX_KEY).unwrap();
        let iv = crate::crypto::hex16(crate::drm::convert::tests::AAX_IV).unwrap();
        let ab: ActivationBytes = "1ceb00da".parse().unwrap();
        let enc = make_encrypted(dir.path(), Some(&payload), &key, &iv);
        let book = book_from(dir.path(), enc, &DecryptionKey::Aax(ab));
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        let drm = ContentDrm::Aaxc(DecryptionKey::Aax(ab));
        let src = open(cdn.url(), drm, &dir.path().join("c.m4b"), small()).await;
        assert_eq!(read_all(&src, 1500).await, book.plain);
    }

    /// Mirrors AVAssetResourceLoader: plain threads blocking on a shared
    /// multi-thread runtime, reading overlapping ranges.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_reads_from_threads() {
        let dir = tempfile::tempdir().unwrap();
        let book = aaxc_book(dir.path());
        let cdn = Cdn::serving(book.encrypted.clone()).await;
        let src = Arc::new(open(cdn.url(), aaxc(), &dir.path().join("c.m4b"), small()).await);
        let plain = Arc::new(book.plain);
        let handle = tokio::runtime::Handle::current();
        let threads: Vec<_> = (0..8u64)
            .map(|t| {
                let (src, plain, handle) = (src.clone(), plain.clone(), handle.clone());
                std::thread::spawn(move || {
                    let n = plain.len() as u64;
                    let piece = 97 + 211 * t as usize;
                    let mut pos = (t * 1013) % n;
                    for _ in 0..40 {
                        let got = handle.block_on(src.read(pos, piece)).unwrap();
                        let end = (pos as usize + piece).min(plain.len());
                        assert_eq!(got, &plain[pos as usize..end]);
                        pos = (pos + piece as u64 / 2 + 1) % n;
                    }
                })
            })
            .collect();
        tokio::task::spawn_blocking(move || {
            for t in threads {
                t.join().unwrap();
            }
        })
        .await
        .unwrap();
        assert!(src.lock().inflight.is_empty());
        assert_eq!(read_all(&src, 4096).await, *plain);
    }

    #[test]
    fn range_set_operations() {
        let mut r = RangeSet::default();
        r.insert(10, 20);
        r.insert(30, 40);
        r.insert(20, 25); // adjacent merges
        assert_eq!(r.0, vec![(10, 25), (30, 40)]);
        assert!(r.covers(10, 25));
        assert!(!r.covers(10, 26));
        assert!(r.covers(5, 5));
        assert_eq!(r.first_gap(0, 50), Some((0, 10)));
        assert_eq!(r.first_gap(12, 50), Some((25, 30)));
        assert_eq!(r.first_gap(12, 28), Some((25, 28)));
        assert_eq!(r.first_gap(31, 40), None);
        assert_eq!(r.first_gap(35, 45), Some((40, 45)));
        assert_eq!(r.next_start(26), Some(30));
        assert_eq!(r.next_start(30), None);
        assert_eq!(r.missing(0, 50), 50 - 15 - 10);
        r.insert(0, 100);
        assert_eq!(r.0, vec![(0, 100)]);
        assert_eq!(r.total(), 100);
        assert_eq!(
            RangeSet::from_vec(vec![(5, 6), (1, 3), (2, 5)]).0,
            vec![(1, 6)]
        );
    }

    #[test]
    fn time_to_sample_mapping() {
        let stts = [(87, 1024), (1, 136)];
        assert_eq!(sample_at_time(&stts, 0), 0);
        assert_eq!(sample_at_time(&stts, 1023), 0);
        assert_eq!(sample_at_time(&stts, 1024), 1);
        assert_eq!(sample_at_time(&stts, 87 * 1024), 87);
        assert_eq!(sample_at_time(&stts, 10_000_000), 88);
    }
}
