//! Main-thread AVPlayer for downloaded audio and streamed books.
//!
//! Every `Player` call happens on the UI thread. AVFoundation publishes item
//! and player state through the main queue, which the app's run loop services,
//! so readiness, buffering and errors show up on the next poll. A streamed
//! source is read on background threads.

/// Bytes of a playable audio file, readable at any offset from any thread.
pub trait StreamRead: Send + Sync + 'static {
    /// Fill `buf` with bytes at `offset`; return how many were written (0 = end of file).
    /// May block on the network for a while. Err = give up this request.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<usize>;
}

/// A seek target that cannot run past the end of a known duration.
fn clamp_seek(seconds: f64, duration: f64) -> f64 {
    let seconds = if seconds.is_finite() {
        seconds.max(0.)
    } else {
        0.
    };
    if duration > 0. {
        seconds.min(duration)
    } else {
        seconds
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::StreamRead;
    use std::os::unix::ffi::OsStrExt;
    use std::{
        ffi::{CStr, CString, c_char, c_void},
        panic::{AssertUnwindSafe, catch_unwind},
        path::Path,
        ptr::NonNull,
        sync::{Arc, Mutex},
    };

    type ReadFn = extern "C" fn(*const c_void, u64, *mut u8, usize) -> i64;
    type ReleaseFn = extern "C" fn(*const c_void);

    unsafe extern "C" {
        fn ba_player_open(path: *const c_char) -> *mut c_void;
        fn ba_player_open_stream(
            ctx: *const c_void,
            read: ReadFn,
            release: ReleaseFn,
            length: u64,
            content_type: *const c_char,
            duration: f64,
        ) -> *mut c_void;
        fn ba_player_close(player: *mut c_void);
        fn ba_player_play(player: *mut c_void) -> bool;
        fn ba_player_pause(player: *mut c_void);
        fn ba_player_position(player: *mut c_void) -> f64;
        fn ba_player_duration(player: *mut c_void) -> f64;
        fn ba_player_seek(player: *mut c_void, time: f64);
        fn ba_player_is_playing(player: *mut c_void) -> bool;
        fn ba_player_is_buffering(player: *mut c_void) -> bool;
        fn ba_player_is_stalled(player: *mut c_void) -> bool;
        fn ba_player_rate(player: *mut c_void) -> f32;
        fn ba_player_set_rate(player: *mut c_void, rate: f32);
        fn ba_player_volume(player: *mut c_void) -> f32;
        fn ba_player_set_volume(player: *mut c_void, volume: f32);
        fn ba_player_error(player: *mut c_void) -> *mut c_char;
        fn free(ptr: *mut c_void);
    }

    /// What the Objective-C side holds for a stream: the reader and its last failure.
    struct StreamContext {
        source: Arc<dyn StreamRead>,
        error: Mutex<Option<String>>,
    }

    impl StreamContext {
        fn fail(&self, message: String) {
            *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
        }
        fn last_error(&self) -> Option<String> {
            self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    /// Called on background queues while a loading request is served.
    extern "C" fn read_stream(ctx: *const c_void, offset: u64, buf: *mut u8, len: usize) -> i64 {
        // SAFETY: ctx is the `Arc<StreamContext>` given to `ba_player_open_stream`, which
        // stays alive until `release_stream`; buf points at `len` writable bytes.
        let ctx = unsafe { &*ctx.cast::<StreamContext>() };
        let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };
        match catch_unwind(AssertUnwindSafe(|| ctx.source.read_at(offset, buf))) {
            Ok(Ok(n)) => i64::try_from(n.min(len)).unwrap_or(i64::MAX),
            Ok(Err(error)) => {
                ctx.fail(format!("{error:#}"));
                -1
            }
            Err(_) => {
                ctx.fail("The audio stream stopped unexpectedly".to_owned());
                -1
            }
        }
    }

    /// Called once, after the player is closed and the last read has returned.
    extern "C" fn release_stream(ctx: *const c_void) {
        // SAFETY: balances the `Arc::into_raw` in `open_stream`.
        drop(unsafe { Arc::from_raw(ctx.cast::<StreamContext>()) });
    }

    pub struct Player {
        handle: NonNull<c_void>,
        stream: Option<Arc<StreamContext>>,
    }

    impl Player {
        pub fn open(path: &Path) -> anyhow::Result<Self> {
            let path = CString::new(path.as_os_str().as_bytes())?;
            // The returned object is retained until Drop; all calls occur on the UI thread.
            let ptr = unsafe { ba_player_open(path.as_ptr()) };
            let handle = NonNull::new(ptr)
                .ok_or_else(|| anyhow::anyhow!("macOS could not open this audio file"))?;
            Ok(Self {
                handle,
                stream: None,
            })
        }

        /// A stream of `length` bytes with UTI `content_type` (e.g. "com.apple.m4a-audio",
        /// "public.mp3") and a known duration in seconds if the caller knows it (else 0.0 / None).
        pub fn open_stream(
            source: Arc<dyn StreamRead>,
            length: u64,
            content_type: &str,
            duration: Option<f64>,
        ) -> anyhow::Result<Self> {
            anyhow::ensure!(length > 0, "The audio stream is empty");
            let content_type = CString::new(content_type)?;
            let stream = Arc::new(StreamContext {
                source,
                error: Mutex::new(None),
            });
            let ctx = Arc::into_raw(stream.clone()).cast::<c_void>();
            // The Objective-C side owns ctx from here and calls release_stream exactly once.
            let ptr = unsafe {
                ba_player_open_stream(
                    ctx,
                    read_stream,
                    release_stream,
                    length,
                    content_type.as_ptr(),
                    duration.unwrap_or(0.),
                )
            };
            let handle = NonNull::new(ptr)
                .ok_or_else(|| anyhow::anyhow!("macOS could not open this audio stream"))?;
            Ok(Self {
                handle,
                stream: Some(stream),
            })
        }

        /// Starts at the chosen rate. Fails if the item has already failed.
        pub fn play(&self) -> anyhow::Result<()> {
            if unsafe { ba_player_play(self.ptr()) } {
                return Ok(());
            }
            Err(anyhow::anyhow!(self.error().unwrap_or_else(|| {
                "macOS could not start audio playback".to_owned()
            })))
        }
        pub fn pause(&self) {
            unsafe { ba_player_pause(self.ptr()) }
        }
        /// Current time, or the target of a seek still in flight.
        pub fn position(&self) -> f64 {
            unsafe { ba_player_position(self.ptr()) }
        }
        /// Seconds, or 0 while a stream without a known duration is still loading.
        pub fn duration(&self) -> f64 {
            unsafe { ba_player_duration(self.ptr()) }
        }
        pub fn seek(&self, seconds: f64) {
            let seconds = super::clamp_seek(seconds, self.duration());
            unsafe { ba_player_seek(self.ptr(), seconds) }
        }
        /// Playback is wanted: playing, or waiting for data.
        pub fn is_playing(&self) -> bool {
            unsafe { ba_player_is_playing(self.ptr()) }
        }
        /// Playback wanted but waiting for data (AVPlayer timeControlStatus == waitingToPlayAtSpecifiedRate).
        pub fn is_buffering(&self) -> bool {
            unsafe { ba_player_is_buffering(self.ptr()) }
        }
        /// The item or player failed (network/decoding); a short human message.
        ///
        /// A stream also reports its last read error while it waits for data
        /// that keeps failing to load; that clears once a read succeeds again.
        pub fn error(&self) -> Option<String> {
            let stream_error = || self.stream.as_ref().and_then(|stream| stream.last_error());
            let ptr = unsafe { ba_player_error(self.ptr()) };
            if ptr.is_null() {
                if !unsafe { ba_player_is_stalled(self.ptr()) } {
                    return None;
                }
                return Some(
                    stream_error().unwrap_or_else(|| "The audio stream stopped loading".to_owned()),
                );
            }
            // SAFETY: a NUL-terminated string from strdup, freed right after copying.
            let message = unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { free(ptr.cast()) };
            Some(stream_error().unwrap_or(message))
        }
        /// The chosen rate, also while paused.
        pub fn rate(&self) -> f32 {
            unsafe { ba_player_rate(self.ptr()) }
        }
        /// Takes effect now when playing; a paused player stays paused.
        pub fn set_rate(&self, rate: f32) {
            unsafe { ba_player_set_rate(self.ptr(), rate.clamp(0.5, 2.0)) }
        }
        pub fn volume(&self) -> f32 {
            unsafe { ba_player_volume(self.ptr()) }
        }
        pub fn set_volume(&self, volume: f32) {
            unsafe { ba_player_set_volume(self.ptr(), volume.clamp(0.0, 1.0)) }
        }
        fn ptr(&self) -> *mut c_void {
            self.handle.as_ptr()
        }
    }
    impl Drop for Player {
        fn drop(&mut self) {
            // Reads still in flight keep the stream context alive until they return.
            unsafe { ba_player_close(self.ptr()) }
        }
    }
}
#[cfg(target_os = "macos")]
pub use platform::Player;

#[cfg(not(target_os = "macos"))]
pub struct Player;
#[cfg(not(target_os = "macos"))]
impl Player {
    pub fn open(_: &std::path::Path) -> anyhow::Result<Self> {
        anyhow::bail!("Playback is currently available on macOS only")
    }
    pub fn open_stream(
        _source: std::sync::Arc<dyn StreamRead>,
        _length: u64,
        _content_type: &str,
        _duration: Option<f64>,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("Playback is currently available on macOS only")
    }
    pub fn play(&self) -> anyhow::Result<()> {
        anyhow::bail!("Playback is currently available on macOS only")
    }
    pub fn pause(&self) {}
    pub fn position(&self) -> f64 {
        0.
    }
    pub fn duration(&self) -> f64 {
        0.
    }
    pub fn seek(&self, _: f64) {}
    pub fn is_playing(&self) -> bool {
        false
    }
    pub fn is_buffering(&self) -> bool {
        false
    }
    pub fn error(&self) -> Option<String> {
        None
    }
    pub fn rate(&self) -> f32 {
        1.0
    }
    pub fn set_rate(&self, _rate: f32) {}
    pub fn volume(&self) -> f32 {
        0.
    }
    pub fn set_volume(&self, _volume: f32) {}
}

#[cfg(test)]
mod clamp_tests {
    use super::clamp_seek;

    #[test]
    fn unknown_duration_does_not_clamp_to_zero() {
        assert_eq!(clamp_seek(42., 0.), 42.);
        assert_eq!(clamp_seek(42., 10.), 10.);
        assert_eq!(clamp_seek(-3., 10.), 0.);
        assert_eq!(clamp_seek(f64::NAN, 10.), 0.);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{Player, StreamRead};
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn write_wav(path: &std::path::Path, seconds: u32, tone: bool) {
        let rate = 16_000u32;
        let samples = rate * seconds;
        let mut data = Vec::with_capacity(samples as usize * 2);
        for i in 0..samples {
            // A quiet tone gives the encoder something real; silence when not asked.
            let sample = if tone {
                (200. * (i as f64 * 440. * std::f64::consts::TAU / f64::from(rate)).sin()) as i16
            } else {
                0
            };
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data.len() as u32).to_le_bytes())
            .unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&(rate * 2).to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        file.write_all(&data).unwrap();
    }

    /// An AAC .m4b of `seconds`, encoded by afconvert (ships with macOS).
    fn generated_m4b(name: &str, seconds: u32) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "better-audible-player-{name}-{}",
            std::process::id()
        ));
        let wav = base.with_extension("wav");
        let m4b = base.with_extension("m4b");
        write_wav(&wav, seconds, true);
        let status = std::process::Command::new("afconvert")
            .args(["-f", "m4bf", "-d", "aac"])
            .arg(&wav)
            .arg(&m4b)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::remove_file(wav).unwrap();
        m4b
    }

    fn m4b_bytes(name: &str, seconds: u32) -> Vec<u8> {
        let path = generated_m4b(name, seconds);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        bytes
    }

    struct Memory {
        bytes: Vec<u8>,
        reads: AtomicUsize,
        delay: Duration,
        failure: Option<&'static str>,
        /// Reads at or past this offset fail.
        fail_from: u64,
        /// Largest piece one read returns.
        max_read: usize,
    }

    impl Memory {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                bytes,
                reads: AtomicUsize::new(0),
                delay: Duration::ZERO,
                failure: None,
                fail_from: u64::MAX,
                max_read: usize::MAX,
            }
        }
    }

    impl StreamRead for Memory {
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<usize> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if let Some(failure) = self.failure {
                anyhow::bail!(failure);
            }
            anyhow::ensure!(offset < self.fail_from, "dropped mid-book");
            let start = (offset as usize).min(self.bytes.len());
            let n = buf.len().min(self.bytes.len() - start).min(self.max_read);
            buf[..n].copy_from_slice(&self.bytes[start..start + n]);
            Ok(n)
        }
    }

    const M4A: &str = "com.apple.m4a-audio";

    // AVFoundation delivers item and player state on the main queue. libtest runs
    // tests on worker threads while its main thread blocks, so these scenarios run
    // in a child copy of the test binary that takes the main thread before libtest
    // starts and spins the main run loop while it waits, like the app does.
    const SCENARIO_ENV: &str = "BETTER_AUDIBLE_PLAYER_SCENARIO";

    #[used]
    #[unsafe(link_section = "__DATA,__mod_init_func")]
    static RUN_SCENARIO: extern "C" fn() = run_scenario_if_asked;

    extern "C" fn run_scenario_if_asked() {
        let Ok(name) = std::env::var(SCENARIO_ENV) else {
            return;
        };
        let scenario: fn() = match name.as_str() {
            "stream_plays" => stream_plays,
            "slow_stream_buffers" => slow_stream_buffers,
            "failing_stream_reports_error" => failing_stream_reports_error,
            "drop_while_reading" => drop_while_reading,
            "file_plays_at_rate" => file_plays_at_rate,
            "stream_drops_mid_book" => stream_drops_mid_book,
            "real_stream_plays" => real_stream_plays,
            _ => std::process::exit(2),
        };
        let ok = std::panic::catch_unwind(scenario).is_ok();
        std::process::exit(if ok { 0 } else { 1 });
    }

    fn on_main_thread(name: &str) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .env(SCENARIO_ENV, name)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "scenario {name} failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopDefaultMode: *const std::ffi::c_void;
        fn CFRunLoopRunInMode(
            mode: *const std::ffi::c_void,
            seconds: f64,
            return_after_source_handled: u8,
        ) -> i32;
    }

    fn pump(seconds: f64) {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 0) };
    }

    fn wait_until(timeout: f64, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < timeout {
            if done() {
                return true;
            }
            pump(0.02);
        }
        done()
    }

    /// Plays a real book from Audible, muted, then seeks far ahead:
    /// `BA_KEY=<account> BA_ASIN=<asin> cargo test -p better-audible real_stream -- --ignored`
    #[test]
    #[ignore = "needs a signed-in account and the network"]
    fn real_stream_plays_and_seeks() {
        on_main_thread("real_stream_plays");
    }

    fn real_stream_plays() {
        let key = std::env::var("BA_KEY").unwrap();
        let asin = audible::Asin::new(std::env::var("BA_ASIN").unwrap()).unwrap();
        let stream =
            crate::streaming::Stream::open(&key, &asin, crate::backend::DownloadQuality::High)
                .unwrap();
        let player = Player::open_stream(
            stream.clone(),
            stream.len(),
            stream.content_type(),
            stream.duration(),
        )
        .unwrap();
        player.set_volume(0.);
        for target in [600., 3000.] {
            player.seek(target);
            player.play().unwrap();
            let started = Instant::now();
            assert!(
                wait_until(30., || player.position() > target + 2.),
                "stuck at {} (error {:?})",
                player.position(),
                player.error()
            );
            eprintln!("{target}: playing after {:?}", started.elapsed());
            player.pause();
        }
        assert!(player.error().is_none());
    }

    fn stream_plays() {
        let bytes = m4b_bytes("stream", 4);
        let memory = Arc::new(Memory::new(bytes));
        let player =
            Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, None).unwrap();
        // Before the item is ready: no duration yet, but a seek still sticks.
        player.seek(1.0);
        assert!((player.position() - 1.0).abs() < 0.01);
        assert!(
            wait_until(10., || player.duration() > 0.),
            "never became ready"
        );
        assert!(
            (player.duration() - 4.).abs() < 0.1,
            "{}",
            player.duration()
        );
        assert!(memory.reads.load(Ordering::SeqCst) > 0);
        pump(0.3);
        assert!(
            (player.position() - 1.0).abs() < 0.05,
            "{}",
            player.position()
        );
        player.seek(2.5);
        assert!((player.position() - 2.5).abs() < 0.01);
        pump(0.3);
        assert!(
            (player.position() - 2.5).abs() < 0.05,
            "{}",
            player.position()
        );
        player.seek(0.5);

        player.set_rate(1.5);
        pump(0.2);
        assert!(!player.is_playing(), "set_rate must not start playback");
        assert!((player.rate() - 1.5).abs() < 0.01);
        player.set_volume(0.3);
        assert!((player.volume() - 0.3).abs() < 0.01);
        player.set_volume(0.);

        player.play().unwrap();
        assert!(player.is_playing());
        let started = Instant::now();
        assert!(
            wait_until(5., || player.position() > 1.1),
            "did not advance"
        );
        let elapsed = started.elapsed().as_secs_f64();
        // Started from the seek target, not from 0.
        assert!(player.position() < 0.5 + elapsed * 1.5 + 0.3);
        assert!((player.rate() - 1.5).abs() < 0.01);
        player.pause();
        assert!(wait_until(2., || !player.is_playing()));
        let paused_at = player.position();
        pump(0.3);
        assert!((player.position() - paused_at).abs() < 0.01);
        assert!(player.error().is_none());
        drop(player);

        // A caller-supplied duration is there at once and clamps seeks.
        let player =
            Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, Some(4.)).unwrap();
        assert!((player.duration() - 4.).abs() < 0.01);
        player.seek(10.);
        assert!((player.position() - 4.).abs() < 0.01);
    }

    fn slow_stream_buffers() {
        let bytes = m4b_bytes("slow", 3);
        let memory = Arc::new(Memory {
            delay: Duration::from_millis(250),
            ..Memory::new(bytes)
        });
        let player =
            Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, Some(3.)).unwrap();
        player.set_volume(0.);
        player.seek(0.5);
        player.play().unwrap();
        let mut saw_buffering = false;
        assert!(
            wait_until(20., || {
                saw_buffering |= player.is_buffering();
                assert!(
                    player.is_playing(),
                    "a buffering stream still wants to play"
                );
                player.position() > 1.2
            }),
            "slow stream never played"
        );
        assert!(saw_buffering, "expected to wait for the first slow reads");
        assert!(player.error().is_none());
    }

    fn failing_stream_reports_error() {
        let memory = Arc::new(Memory {
            failure: Some("network is down"),
            ..Memory::new(vec![0; 64 * 1024])
        });
        let player =
            Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, None).unwrap();
        let _ = player.play();
        assert!(wait_until(15., || player.error().is_some()), "no error");
        assert_eq!(player.error().as_deref(), Some("network is down"));
        // The item itself fails soon after; then play() refuses.
        assert!(
            wait_until(15., || player.play().is_err()),
            "item never failed"
        );
        assert_eq!(player.error().as_deref(), Some("network is down"));
        assert!(!player.is_buffering());
        assert!(!player.is_playing());
    }

    fn stream_drops_mid_book() {
        // About four minutes of audio; only the first ~40 s can be read.
        let memory = Arc::new(Memory {
            fail_from: 128 * 1024,
            max_read: 64 * 1024,
            ..Memory::new(m4b_bytes("drops", 240))
        });
        let player =
            Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, Some(240.))
                .unwrap();
        player.set_volume(0.);
        player.set_rate(2.);
        player.play().unwrap();
        // AVPlayer would wait forever after the failed read; what loaded still plays.
        assert!(
            wait_until(10., || player.position() > 3.),
            "buffered audio did not play: {:?}",
            player.error()
        );
        player.seek(38.);
        assert!(
            wait_until(15., || player.is_buffering() && player.error().is_some()),
            "stall was not reported"
        );
        assert_eq!(player.error().as_deref(), Some("dropped mid-book"));
        assert!(player.is_playing(), "still wants to play");
        assert!(player.position() < 50., "skipped past the loaded audio");
    }

    fn drop_while_reading() {
        let bytes = m4b_bytes("drop", 3);
        let memory = Arc::new(Memory {
            delay: Duration::from_millis(150),
            ..Memory::new(bytes)
        });
        for round in 0..5 {
            let player =
                Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, None).unwrap();
            player.set_volume(0.);
            player.play().unwrap();
            let reads = memory.reads.load(Ordering::SeqCst);
            wait_until(2., || memory.reads.load(Ordering::SeqCst) > reads);
            pump(0.05 * f64::from(round));
            drop(player);
            // The context goes once the last in-flight read returns.
            assert!(
                wait_until(5., || Arc::strong_count(&memory) == 1),
                "stream context leaked in round {round}"
            );
        }
    }

    fn file_plays_at_rate() {
        let path = generated_m4b("file", 4);
        let player = Player::open(&path).unwrap();
        player.set_volume(0.);
        player.set_rate(2.0);
        player.seek(1.0);
        player.play().unwrap();
        assert!(player.is_playing());
        let started = Instant::now();
        assert!(
            wait_until(5., || player.position() > 2.0),
            "did not advance"
        );
        let elapsed = started.elapsed().as_secs_f64();
        assert!(player.position() < 1.0 + elapsed * 2. + 0.3);
        // Runs off the end and pauses there.
        assert!(
            wait_until(5., || !player.is_playing()),
            "did not stop at the end"
        );
        assert!((player.position() - player.duration()).abs() < 0.1);
        drop(player);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn streams_from_memory() {
        on_main_thread("stream_plays");
    }

    #[test]
    fn slow_stream_waits_then_plays() {
        on_main_thread("slow_stream_buffers");
    }

    #[test]
    fn failing_stream_reports_its_error() {
        on_main_thread("failing_stream_reports_error");
    }

    #[test]
    fn dropping_while_reads_are_in_flight_is_safe() {
        on_main_thread("drop_while_reading");
    }

    #[test]
    fn stream_that_drops_mid_book_plays_what_loaded_then_reports() {
        on_main_thread("stream_drops_mid_book");
    }

    #[test]
    fn plays_a_local_file_at_the_chosen_rate() {
        on_main_thread("file_plays_at_rate");
    }

    #[test]
    fn stream_seek_sticks_without_a_run_loop() {
        let memory = Arc::new(Memory::new(m4b_bytes("seek", 1)));
        let player =
            Player::open_stream(memory.clone(), memory.bytes.len() as u64, M4A, None).unwrap();
        assert!(player.duration() == 0.);
        player.seek(30.);
        assert!((player.position() - 30.).abs() < 0.01);
        assert!(!player.is_playing());
        assert!(player.error().is_none());
        player.set_rate(0.8);
        assert!((player.rate() - 0.8).abs() < 0.01);
    }

    #[test]
    fn rejects_an_empty_stream() {
        let memory = Arc::new(Memory::new(Vec::new()));
        assert!(Player::open_stream(memory, 0, M4A, None).is_err());
    }

    #[test]
    fn rejects_a_file_that_is_not_audio() {
        let path = std::env::temp_dir().join(format!(
            "better-audible-player-junk-{}.m4b",
            std::process::id()
        ));
        std::fs::write(&path, b"not audio at all").unwrap();
        assert!(Player::open(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn opens_a_generated_m4b_when_ffmpeg_is_available() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let path =
            std::env::temp_dir().join(format!("better-audible-player-{}.m4b", std::process::id()));
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=mono",
                "-t",
                "1",
                "-c:a",
                "aac",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let player = Player::open(&path).unwrap();
        assert!((player.duration() - 1.).abs() < 0.2);
        player.seek(0.5);
        assert!((player.position() - 0.5).abs() < 0.1);
        drop(player);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn opens_and_seeks_a_local_pcm_file() {
        let path =
            std::env::temp_dir().join(format!("better-audible-player-{}.wav", std::process::id()));
        write_wav(&path, 1, false);
        let player = Player::open(&path).unwrap();
        assert!((player.duration() - 1.).abs() < 0.05);
        player.seek(0.5);
        assert!((player.position() - 0.5).abs() < 0.05);
        player.set_rate(1.25);
        assert!((player.rate() - 1.25).abs() < 0.01);
        assert!(!player.is_playing());
        player.set_volume(0.4);
        assert!((player.volume() - 0.4).abs() < 0.01);
        drop(player);
        std::fs::remove_file(path).unwrap();
    }
}
