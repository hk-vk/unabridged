//! User-facing time and number copy. Plain, sentence case, no exclamation marks.

use std::time::Duration;

use crate::book::TimeOfDay;
use crate::progress::ChapterProgress;

/// U+2212 MINUS SIGN, for remaining-time readouts.
const MINUS: char = '\u{2212}';
/// U+00D7 MULTIPLICATION SIGN, for playback speed.
const TIMES: char = '\u{00D7}';

const SECS_PER_MIN: u64 = 60;
const SECS_PER_HOUR: u64 = 3600;
const SECS_PER_DAY: u64 = 86_400;
const DAYS_PER_WEEK: u64 = 7;

/// "8:57", "14:02", "1:04:09": a clock readout, hours only when needed.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (
        s / SECS_PER_HOUR,
        s % SECS_PER_HOUR / SECS_PER_MIN,
        s % SECS_PER_MIN,
    );
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "−8:57": time remaining, with a true minus sign.
pub fn countdown(d: Duration) -> String {
    format!("{MINUS}{}", clock(d))
}

/// "9 hr 12 min", "47 min", "9 hr", "under 1 min".
pub fn hours_minutes(d: Duration) -> String {
    let total_min = d.as_secs() / SECS_PER_MIN;
    let (h, m) = (total_min / 60, total_min % 60);
    match (h, m) {
        (0, 0) => "under 1 min".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

/// "9 hr 12 min left".
pub fn time_left(d: Duration) -> String {
    format!("{} left", hours_minutes(d))
}

/// Whole hours, rounded to nearest (at least 1 when any time remains).
pub fn rounded_hours(d: Duration) -> u64 {
    let hours = (d.as_secs() + SECS_PER_HOUR / 2) / SECS_PER_HOUR;
    if hours == 0 && !d.is_zero() { 1 } else { hours }
}

/// "9 hr left in book", for the compact popover footer.
pub fn book_left_short(d: Duration) -> String {
    if d < Duration::from_secs(SECS_PER_HOUR) {
        format!("{} left in book", hours_minutes(d))
    } else {
        format!("{} hr left in book", rounded_hours(d))
    }
}

/// "About 9 hours left in the book".
pub fn book_left_long(d: Duration) -> String {
    if d < Duration::from_secs(SECS_PER_HOUR) {
        format!("{} left in the book", capitalize(&hours_minutes(d)))
    } else {
        format!(
            "About {} left in the book",
            plural(rounded_hours(d), "hour")
        )
    }
}

/// "43%".
pub fn percent(value: u32) -> String {
    format!("{value}%")
}

/// "1.25×", "1.3×", "1×": up to two decimals, without trailing zeros, so
/// presets like 1.25 are not shown as their rounded neighbour.
pub fn speed(rate: f32) -> String {
    let text = format!("{:.2}", (rate * 100.0).round() / 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}{TIMES}")
}

/// "9:32 PM".
pub fn time_of_day(t: TimeOfDay) -> String {
    let (hour12, suffix) = match t.hour {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!("{hour12}:{:02} {suffix}", t.minute)
}

/// Days of the week, Monday first.
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// How long ago something happened, relative to `today` (0 = Monday):
/// "just now", "41 minutes ago", "3 hours ago", "yesterday", "Tuesday",
/// "last week", "3 weeks ago".
pub fn ago(age: Duration, today: usize) -> String {
    let secs = age.as_secs();
    let days = secs / SECS_PER_DAY;
    match secs {
        s if s < SECS_PER_MIN => "just now".into(),
        s if s < SECS_PER_HOUR => format!("{} ago", plural(s / SECS_PER_MIN, "minute")),
        s if s < SECS_PER_DAY => format!("{} ago", plural(s / SECS_PER_HOUR, "hour")),
        _ if days == 1 => "yesterday".into(),
        _ if days < DAYS_PER_WEEK => WEEKDAYS[(today + 7 - (days as usize % 7)) % 7].to_string(),
        _ if days < 2 * DAYS_PER_WEEK => "last week".into(),
        _ => format!("{} weeks ago", days / DAYS_PER_WEEK),
    }
}

/// "Chapter 14 of 32, The Glass Orchard".
pub fn chapter_of(p: &ChapterProgress) -> String {
    let base = format!("Chapter {} of {}", p.number(), p.count);
    with_title(base, &p.chapter.title)
}

/// "Chapter 14, The Glass Orchard".
pub fn chapter_short(p: &ChapterProgress) -> String {
    with_title(format!("Chapter {}", p.number()), &p.chapter.title)
}

fn with_title(base: String, title: &str) -> String {
    if title.is_empty() {
        base
    } else {
        format!("{base}, {title}")
    }
}

/// "left in The Glass Orchard, chapter 14 of 32."
pub fn left_in_chapter_caption(p: &ChapterProgress) -> String {
    let place = if p.chapter.title.is_empty() {
        format!("chapter {}", p.number())
    } else {
        p.chapter.title.clone()
    };
    format!("left in {place}, chapter {} of {}.", p.number(), p.count)
}

/// "Nine minutes left in The Glass Orchard, and about nine hours in the book."
pub fn listening_sentence(p: &ChapterProgress) -> String {
    let chapter_min = p.chapter_remaining.as_secs().div_ceil(SECS_PER_MIN);
    let chapter_part = if chapter_min <= 1 {
        "Less than a minute".to_string()
    } else {
        capitalize(&spelled_plural(chapter_min, "minute"))
    };
    let place = if p.chapter.title.is_empty() {
        format!("chapter {}", p.number())
    } else {
        p.chapter.title.clone()
    };
    let book_part = if p.book_remaining < Duration::from_secs(SECS_PER_HOUR) {
        format!(
            "{} in the book",
            spelled_plural(p.book_remaining.as_secs() / SECS_PER_MIN, "minute")
        )
    } else {
        format!(
            "about {} in the book",
            spelled_plural(rounded_hours(p.book_remaining), "hour")
        )
    };
    format!("{chapter_part} left in {place}, and {book_part}.")
}

/// "5 books, about 31 hours left in total".
pub fn library_summary(books: usize, remaining: Duration) -> String {
    format!(
        "{}, about {} left in total",
        plural(books as u64, "book"),
        plural(rounded_hours(remaining), "hour")
    )
}

/// "1 minute", "41 minutes".
pub fn plural(n: u64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// Like [`plural`] but spells out numbers up to twenty: "nine hours".
pub fn spelled_plural(n: u64, unit: &str) -> String {
    const WORDS: [&str; 21] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    let number = WORDS
        .get(n as usize)
        .map(|w| w.to_string())
        .unwrap_or_else(|| n.to_string());
    if n == 1 {
        format!("{number} {unit}")
    } else {
        format!("{number} {unit}s")
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, BookId, Chapter};

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(Duration::from_secs(842)), "14:02");
        assert_eq!(clock(Duration::from_secs(537)), "8:57");
        assert_eq!(clock(Duration::from_secs(3849)), "1:04:09");
        assert_eq!(countdown(Duration::from_secs(537)), "\u{2212}8:57");
    }

    #[test]
    fn hours_and_minutes() {
        assert_eq!(time_left(mins(9 * 60 + 12)), "9 hr 12 min left");
        assert_eq!(time_left(mins(47)), "47 min left");
        assert_eq!(hours_minutes(mins(9 * 60)), "9 hr");
        assert_eq!(hours_minutes(Duration::from_secs(30)), "under 1 min");
    }

    #[test]
    fn book_left_copy() {
        assert_eq!(book_left_short(mins(9 * 60 + 12)), "9 hr left in book");
        assert_eq!(
            book_left_long(mins(9 * 60 + 12)),
            "About 9 hours left in the book"
        );
        assert_eq!(book_left_long(mins(47)), "47 min left in the book");
    }

    #[test]
    fn speeds() {
        assert_eq!(speed(1.3), "1.3\u{d7}");
        assert_eq!(speed(1.0), "1\u{d7}");
        assert_eq!(speed(2.0), "2\u{d7}");
        assert_eq!(speed(1.25), "1.25\u{d7}");
        assert_eq!(speed(0.75), "0.75\u{d7}");
        assert_eq!(speed(1.1), "1.1\u{d7}");
    }

    #[test]
    fn times_of_day() {
        assert_eq!(
            time_of_day(TimeOfDay {
                hour: 21,
                minute: 32
            }),
            "9:32 PM"
        );
        assert_eq!(time_of_day(TimeOfDay { hour: 0, minute: 5 }), "12:05 AM");
        assert_eq!(
            time_of_day(TimeOfDay {
                hour: 12,
                minute: 0
            }),
            "12:00 PM"
        );
    }

    #[test]
    fn relative_ages() {
        const SUNDAY: usize = 6;
        let day = Duration::from_secs(SECS_PER_DAY);
        assert_eq!(ago(Duration::from_secs(5), SUNDAY), "just now");
        assert_eq!(ago(mins(41), SUNDAY), "41 minutes ago");
        assert_eq!(ago(mins(1), SUNDAY), "1 minute ago");
        assert_eq!(ago(mins(180), SUNDAY), "3 hours ago");
        assert_eq!(ago(day, SUNDAY), "yesterday");
        assert_eq!(ago(day * 5, SUNDAY), "Tuesday");
        assert_eq!(ago(day * 3, 0), "Friday");
        assert_eq!(ago(day * 9, SUNDAY), "last week");
        assert_eq!(ago(day * 22, SUNDAY), "3 weeks ago");
    }

    fn progress(
        chapter_len: u64,
        into: u64,
        chapter_title: &str,
        book_left_min: u64,
    ) -> ChapterProgress {
        let mut chapters = Vec::new();
        let mut start = Duration::ZERO;
        for i in 0..32 {
            let duration = if i == 13 {
                Duration::from_secs(chapter_len)
            } else {
                mins(30)
            };
            chapters.push(Chapter {
                title: if i == 13 {
                    chapter_title.into()
                } else {
                    String::new()
                },
                start,
                duration,
            });
            start += duration;
        }
        let book = Book {
            id: BookId::new("x"),
            title: "X".into(),
            authors: vec![],
            narrators: vec![],
            cover: String::new(),
            chapters,
            genre: None,
        };
        let pos = book.runtime() - mins(book_left_min);
        let mut p = ChapterProgress::locate(&book, pos).unwrap();
        // Pin the chapter readouts for the sentence under test.
        p.index = 13;
        p.chapter = book.chapters[13].clone();
        p.chapter_elapsed = Duration::from_secs(into);
        p.chapter_remaining = Duration::from_secs(chapter_len - into);
        p
    }

    #[test]
    fn listening_sentence_matches_the_design() {
        let p = progress(1379, 842, "The Glass Orchard", 9 * 60 + 12);
        assert_eq!(
            listening_sentence(&p),
            "Nine minutes left in The Glass Orchard, and about nine hours in the book."
        );
        assert_eq!(chapter_of(&p), "Chapter 14 of 32, The Glass Orchard");
        assert_eq!(chapter_short(&p), "Chapter 14, The Glass Orchard");
        assert_eq!(
            left_in_chapter_caption(&p),
            "left in The Glass Orchard, chapter 14 of 32."
        );
    }

    #[test]
    fn untitled_chapters_fall_back_to_numbers() {
        let p = progress(1379, 1350, "", 30);
        assert_eq!(
            listening_sentence(&p),
            "Less than a minute left in chapter 14, and 30 minutes in the book."
        );
        assert_eq!(chapter_of(&p), "Chapter 14 of 32");
    }

    #[test]
    fn summaries() {
        assert_eq!(
            library_summary(5, mins(30 * 60 + 46)),
            "5 books, about 31 hours left in total"
        );
        assert_eq!(spelled_plural(1, "hour"), "one hour");
        assert_eq!(spelled_plural(31, "hour"), "31 hours");
    }
}
