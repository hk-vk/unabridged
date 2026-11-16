//! Where a position falls among a book's chapters.

use std::time::Duration;

use crate::book::{Book, Chapter};

/// A position resolved against the chapter list.
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterProgress {
    /// Zero-based index of the current chapter.
    pub index: usize,
    /// Number of chapters.
    pub count: usize,
    /// The current chapter.
    pub chapter: Chapter,
    /// Time into the current chapter.
    pub chapter_elapsed: Duration,
    /// Time left in the current chapter.
    pub chapter_remaining: Duration,
    /// Time left in the book.
    pub book_remaining: Duration,
    /// Fraction of the book played, 0..=1.
    pub book_fraction: f32,
}

impl ChapterProgress {
    /// Resolves `position` in `book`. `None` if the book has no chapters.
    /// Positions past the end clamp to the end of the last chapter.
    pub fn locate(book: &Book, position: Duration) -> Option<Self> {
        let runtime = book.runtime();
        let position = position.min(runtime);
        let index = book
            .chapters
            .iter()
            .position(|c| position < c.end())
            .unwrap_or(book.chapters.len().checked_sub(1)?);
        let chapter = book.chapters[index].clone();
        let chapter_elapsed = position.saturating_sub(chapter.start).min(chapter.duration);
        Some(Self {
            index,
            count: book.chapters.len(),
            chapter_remaining: chapter.duration - chapter_elapsed,
            chapter,
            chapter_elapsed,
            book_remaining: runtime - position,
            book_fraction: if runtime.is_zero() {
                0.0
            } else {
                position.as_secs_f32() / runtime.as_secs_f32()
            },
        })
    }

    /// One-based chapter number.
    pub fn number(&self) -> usize {
        self.index + 1
    }

    /// Fraction of the current chapter played, 0..=1.
    pub fn chapter_fraction(&self) -> f32 {
        if self.chapter.duration.is_zero() {
            0.0
        } else {
            self.chapter_elapsed.as_secs_f32() / self.chapter.duration.as_secs_f32()
        }
    }

    /// Segments for a chapter progress bar.
    pub fn segments(&self) -> Segments {
        Segments {
            done: self.index as f32,
            current: self.chapter_fraction(),
            rest: (self.count - self.index - 1) as f32,
        }
    }

    /// Whole percent of the book played, never 100 until the end.
    pub fn percent(&self) -> u32 {
        let pct = (self.book_fraction * 100.0).floor() as u32;
        if self.book_remaining.is_zero() {
            100
        } else {
            pct.min(99)
        }
    }
}

/// Weights of the three parts of a chapter progress bar: finished chapters,
/// the current chapter's played fraction, and chapters still to come.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segments {
    /// Chapters finished.
    pub done: f32,
    /// Fraction of the current chapter played.
    pub current: f32,
    /// Chapters after the current one.
    pub rest: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::BookId;

    fn book(minutes: &[u64]) -> Book {
        let mut start = Duration::ZERO;
        let chapters = minutes
            .iter()
            .map(|&m| {
                let c = Chapter {
                    title: String::new(),
                    start,
                    duration: Duration::from_secs(m * 60),
                };
                start += c.duration;
                c
            })
            .collect();
        Book {
            id: BookId::new("b"),
            title: "B".into(),
            authors: vec![],
            narrators: vec![],
            cover: String::new(),
            chapters,
            genre: None,
        }
    }

    #[test]
    fn locates_position_inside_a_chapter() {
        let b = book(&[10, 20, 30]);
        let p = ChapterProgress::locate(&b, Duration::from_secs(15 * 60)).unwrap();
        assert_eq!(p.number(), 2);
        assert_eq!(p.chapter_elapsed, Duration::from_secs(5 * 60));
        assert_eq!(p.chapter_remaining, Duration::from_secs(15 * 60));
        assert_eq!(p.book_remaining, Duration::from_secs(45 * 60));
        assert_eq!(p.percent(), 25);
        let s = p.segments();
        assert_eq!((s.done, s.rest), (1.0, 1.0));
        assert!((s.current - 0.25).abs() < 1e-6);
    }

    #[test]
    fn chapter_boundaries_belong_to_the_next_chapter() {
        let b = book(&[10, 20]);
        let p = ChapterProgress::locate(&b, Duration::from_secs(10 * 60)).unwrap();
        assert_eq!(p.index, 1);
        assert_eq!(p.chapter_elapsed, Duration::ZERO);
    }

    #[test]
    fn positions_past_the_end_clamp() {
        let b = book(&[10, 20]);
        let p = ChapterProgress::locate(&b, Duration::from_secs(99 * 60)).unwrap();
        assert_eq!(p.index, 1);
        assert_eq!(p.chapter_remaining, Duration::ZERO);
        assert_eq!(p.percent(), 100);
        assert_eq!(p.segments().rest, 0.0);
    }

    #[test]
    fn nearly_finished_books_read_99_percent() {
        let b = book(&[100]);
        let p = ChapterProgress::locate(&b, Duration::from_secs(100 * 60 - 1)).unwrap();
        assert_eq!(p.percent(), 99);
    }

    #[test]
    fn books_without_chapters_have_no_progress() {
        assert!(ChapterProgress::locate(&book(&[]), Duration::ZERO).is_none());
    }
}
