//! A silent playback engine whose position advances with wall time.

use std::time::{Duration, Instant};

use crate::book::Book;
use crate::provider::PlaybackEngine;

/// Source of the current instant, injectable for tests.
pub trait Clock: Send + 'static {
    /// Now.
    fn now(&self) -> Instant;
}

/// The real monotonic clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Plays nothing, but keeps time exactly like a real engine would: the
/// position advances at `speed` while playing and stops at the end of the book.
#[derive(Debug)]
pub struct MockEngine<C: Clock = SystemClock> {
    clock: C,
    runtime: Duration,
    /// Position when playback last started or was moved.
    anchor: Duration,
    /// When playback last started; `None` while paused.
    playing_since: Option<Instant>,
    speed: f32,
    volume: f32,
}

/// Default output volume of the mock.
const DEFAULT_VOLUME: f32 = 0.64;

impl Default for MockEngine<SystemClock> {
    fn default() -> Self {
        Self::with_clock(SystemClock)
    }
}

impl<C: Clock> MockEngine<C> {
    /// A paused, empty engine reading time from `clock`.
    pub fn with_clock(clock: C) -> Self {
        Self {
            clock,
            runtime: Duration::ZERO,
            anchor: Duration::ZERO,
            playing_since: None,
            speed: 1.0,
            volume: DEFAULT_VOLUME,
        }
    }

    /// Folds elapsed play time into the anchor.
    fn settle(&mut self) {
        self.anchor = self.position();
        if self.playing_since.is_some() {
            self.playing_since = Some(self.clock.now());
        }
    }
}

impl<C: Clock> PlaybackEngine for MockEngine<C> {
    fn load(&mut self, book: &Book, position: Duration) {
        self.runtime = book.runtime();
        self.anchor = position.min(self.runtime);
        self.playing_since = None;
    }

    fn play(&mut self) {
        if self.playing_since.is_none() && self.anchor < self.runtime {
            self.playing_since = Some(self.clock.now());
        }
    }

    fn pause(&mut self) {
        self.settle();
        self.playing_since = None;
    }

    fn is_playing(&self) -> bool {
        self.playing_since.is_some() && self.position() < self.runtime
    }

    fn position(&self) -> Duration {
        let played = self
            .playing_since
            .map(|since| self.clock.now().duration_since(since).mul_f32(self.speed))
            .unwrap_or_default();
        (self.anchor + played).min(self.runtime)
    }

    fn seek(&mut self, position: Duration) {
        self.settle();
        self.anchor = position.min(self.runtime);
    }

    fn speed(&self) -> f32 {
        self.speed
    }

    fn set_speed(&mut self, speed: f32) {
        self.settle();
        self.speed = speed;
    }

    fn volume(&self) -> f32 {
        self.volume
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BookId, Chapter};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FakeClock(Arc<Mutex<Instant>>);

    impl FakeClock {
        fn advance(&self, d: Duration) {
            *self.0.lock().unwrap() += d;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }

    fn book(minutes: u64) -> Book {
        Book {
            id: BookId::new("b"),
            title: "B".into(),
            authors: vec![],
            narrators: vec![],
            cover: String::new(),
            chapters: vec![Chapter {
                title: String::new(),
                start: Duration::ZERO,
                duration: Duration::from_secs(minutes * 60),
            }],
            genre: None,
        }
    }

    fn engine() -> (MockEngine<FakeClock>, FakeClock) {
        let clock = FakeClock(Arc::new(Mutex::new(Instant::now())));
        let mut engine = MockEngine::with_clock(clock.clone());
        engine.load(&book(10), Duration::from_secs(60));
        (engine, clock)
    }

    #[test]
    fn position_advances_only_while_playing() {
        let (mut e, clock) = engine();
        clock.advance(Duration::from_secs(5));
        assert_eq!(e.position(), Duration::from_secs(60));
        e.play();
        clock.advance(Duration::from_secs(5));
        assert_eq!(e.position(), Duration::from_secs(65));
        e.pause();
        clock.advance(Duration::from_secs(5));
        assert_eq!(e.position(), Duration::from_secs(65));
    }

    #[test]
    fn speed_scales_progress_from_the_change_onwards() {
        let (mut e, clock) = engine();
        e.play();
        clock.advance(Duration::from_secs(10));
        e.set_speed(2.0);
        clock.advance(Duration::from_secs(10));
        assert_eq!(e.position(), Duration::from_secs(60 + 10 + 20));
    }

    #[test]
    fn skips_and_seeks_clamp_to_the_book() {
        let (mut e, _) = engine();
        e.skip(-90);
        assert_eq!(e.position(), Duration::ZERO);
        e.skip(30);
        assert_eq!(e.position(), Duration::from_secs(30));
        e.seek(Duration::from_secs(99_999));
        assert_eq!(e.position(), Duration::from_secs(600));
    }

    #[test]
    fn playback_stops_at_the_end() {
        let (mut e, clock) = engine();
        e.play();
        clock.advance(Duration::from_secs(3600));
        assert_eq!(e.position(), Duration::from_secs(600));
        assert!(!e.is_playing());
        e.toggle();
        assert!(!e.is_playing(), "cannot play past the end");
    }
}
