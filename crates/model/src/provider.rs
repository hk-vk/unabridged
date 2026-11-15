//! The seams between the UI and a backend.
//!
//! The app only talks to these traits. [`crate::mock`] implements them with
//! sample data; the Audible client (`crates/audible`) will implement them for
//! real accounts. Methods are blocking: the app calls them on GPUI's
//! background executor, so a network-backed implementation may block.

use std::time::Duration;

use crate::book::{Account, Book, CatalogItem, ExploreFeed, Library};

/// Read access to the account, library and store.
pub trait LibraryProvider: Send + Sync + 'static {
    /// The signed-in account.
    fn account(&self) -> anyhow::Result<Account>;

    /// The user's library with listening progress.
    fn library(&self) -> anyhow::Result<Library>;

    /// The Explore feed.
    fn explore(&self) -> anyhow::Result<ExploreFeed>;

    /// Catalogue search.
    fn search_catalog(&self, query: &str) -> anyhow::Result<Vec<CatalogItem>>;
}

/// When playback should stop on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SleepTimer {
    /// Never.
    #[default]
    Off,
    /// At the end of the current chapter.
    EndOfChapter,
    /// After a number of minutes.
    Minutes(u32),
}

impl SleepTimer {
    /// The cycle the sleep control steps through.
    pub const CYCLE: [SleepTimer; 5] = [
        SleepTimer::Off,
        SleepTimer::EndOfChapter,
        SleepTimer::Minutes(15),
        SleepTimer::Minutes(30),
        SleepTimer::Minutes(60),
    ];

    /// "Stop after this chapter", "Stop in 15 min", "Sleep timer off".
    pub fn label(self) -> String {
        match self {
            SleepTimer::Off => "Sleep timer off".into(),
            SleepTimer::EndOfChapter => "Stop after this chapter".into(),
            SleepTimer::Minutes(m) => format!("Stop in {m} min"),
        }
    }

    /// The next setting in [`Self::CYCLE`].
    pub fn next(self) -> Self {
        let ix = Self::CYCLE.iter().position(|t| *t == self).unwrap_or(0);
        Self::CYCLE[(ix + 1) % Self::CYCLE.len()]
    }
}

/// Playback speeds offered by the speed control.
pub const SPEEDS: [f32; 7] = [0.8, 1.0, 1.1, 1.2, 1.3, 1.5, 2.0];

/// The next speed after `current` in [`SPEEDS`], wrapping.
pub fn next_speed(current: f32) -> f32 {
    SPEEDS
        .iter()
        .copied()
        .find(|s| *s > current + f32::EPSILON)
        .unwrap_or(SPEEDS[0])
}

/// Audio playback. Implementations own decoding and output; media keys and the
/// Now Playing centre are routed to these methods by the app.
pub trait PlaybackEngine: Send + 'static {
    /// Loads `book` and moves to `position`, paused.
    fn load(&mut self, book: &Book, position: Duration);

    /// Starts or resumes playback.
    fn play(&mut self);

    /// Pauses playback.
    fn pause(&mut self);

    /// Whether audio is playing.
    fn is_playing(&self) -> bool;

    /// Current position in the book.
    fn position(&self) -> Duration;

    /// Moves to `position` (clamped to the book).
    fn seek(&mut self, position: Duration);

    /// Playback rate (1.0 is normal).
    fn speed(&self) -> f32;

    /// Sets the playback rate.
    fn set_speed(&mut self, speed: f32);

    /// Output volume, 0..=1.
    fn volume(&self) -> f32;

    /// Sets the output volume.
    fn set_volume(&mut self, volume: f32);

    /// Toggles between playing and paused.
    fn toggle(&mut self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Skips by `seconds` (negative skips back).
    fn skip(&mut self, seconds: i64) {
        let now = self.position();
        let target = if seconds.is_negative() {
            now.saturating_sub(Duration::from_secs(seconds.unsigned_abs()))
        } else {
            now + Duration::from_secs(seconds.unsigned_abs())
        };
        self.seek(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_timer_cycles() {
        assert_eq!(SleepTimer::Off.next(), SleepTimer::EndOfChapter);
        assert_eq!(SleepTimer::Minutes(60).next(), SleepTimer::Off);
        assert_eq!(SleepTimer::EndOfChapter.label(), "Stop after this chapter");
    }

    #[test]
    fn speed_cycles_and_wraps() {
        assert_eq!(next_speed(1.0), 1.1);
        assert_eq!(next_speed(1.3), 1.5);
        assert_eq!(next_speed(2.0), 0.8);
    }
}
