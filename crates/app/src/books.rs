//! Pure helpers over library data: progress, chapters, download stages, blurbs.

/// Plain text from Audible's HTML blurbs: tags dropped, entities decoded,
/// whitespace collapsed. Only the tests use it now; blurbs go through
/// `blurb_paragraphs`.
#[cfg(test)]
pub(crate) fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A publisher summary as plain paragraphs: block tags (`p`, `br`, `div`,
/// `li`) start a new paragraph, and divider lines such as `________` are
/// dropped.
pub(crate) fn blurb_paragraphs(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut tag: Option<String> = None;
    for ch in html.chars() {
        match (&mut tag, ch) {
            (None, '<') => tag = Some(String::new()),
            (Some(name), '>') => {
                let name = name
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let block = matches!(name.as_str(), "p" | "br" | "div" | "li" | "ul" | "ol");
                text.push(if block { '\n' } else { ' ' });
                tag = None;
            }
            (Some(name), _) => name.push(ch),
            (None, _) => text.push(ch),
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.split('\n')
        .flat_map(|line| line.split("___"))
        .map(|part| {
            part.split_whitespace()
                .filter(|word| !word.chars().all(|c| c == '_'))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|part| part.chars().any(char::is_alphanumeric))
        .collect()
}

pub(crate) fn download_display(
    event: audible::DownloadEvent,
) -> (&'static str, String, Option<f32>) {
    fn megabytes(bytes: u64) -> f64 {
        bytes as f64 / 1_000_000.0
    }
    match event {
        audible::DownloadEvent::Licensing => (
            "Preparing download",
            "Requesting the book from Audible".into(),
            None,
        ),
        audible::DownloadEvent::Downloading(progress) => {
            let amount = match progress.total {
                Some(total) if total > 0 => {
                    format!(
                        "{:.1} of {:.1} MB",
                        megabytes(progress.downloaded),
                        megabytes(total)
                    )
                }
                _ => format!("{:.1} MB downloaded", megabytes(progress.downloaded)),
            };
            let fraction = progress.fraction().map(|value| value as f32);
            let detail = fraction.map_or(amount.clone(), |value| {
                format!("{:.0}% · {amount}", value * 100.)
            });
            ("Downloading", detail, fraction)
        }
        audible::DownloadEvent::Decrypting(progress) => {
            let fraction = progress.fraction().map(|value| value as f32);
            let detail = fraction.map_or_else(
                || "Finishing the audio file".into(),
                |value| format!("{:.0}% · Finishing the audio file", value * 100.),
            );
            ("Preparing audio", detail, fraction)
        }
    }
}

pub(crate) fn book_finished(item: &audible::models::LibraryItem) -> bool {
    item.is_finished
        .or(item
            .listening_status
            .as_ref()
            .and_then(|status| status.is_finished))
        .unwrap_or(false)
}

pub(crate) fn book_progress(item: &audible::models::LibraryItem) -> Option<f64> {
    item.percent_complete
        .or(item
            .listening_status
            .as_ref()
            .and_then(|status| status.percent_complete))
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0., 100.))
}

/// Puts a listening place into the item's percent and time left, so every
/// view shows where the listener is on this Mac rather than Audible's last
/// figure. `length_ms` is the book's exact length when known (the player's);
/// otherwise the catalog runtime is used. Time left is at 1×, as Audible
/// shows it.
pub(crate) fn apply_position(
    item: &mut audible::models::LibraryItem,
    millis: u64,
    length_ms: Option<u64>,
) {
    let Some(length) = length_ms
        .or(item
            .product
            .runtime_length_min
            .map(|minutes| u64::from(minutes) * 60_000))
        .filter(|length| *length > 0)
    else {
        return;
    };
    let position = millis.min(length);
    let percent = position as f64 / length as f64 * 100.;
    item.percent_complete = Some(percent);
    let status = item.listening_status.get_or_insert_with(Default::default);
    status.percent_complete = Some(percent);
    status.time_remaining_seconds = Some((length - position) / 1000);
}

/// Every chapter in playing order, parts and sections included. Audible
/// gives a chapter with sections its own stretch before the first section
/// ("Reliability", then "Hardware Faults"); keeping only the innermost ones
/// left those stretches in no chapter, and the player lost its chapter there.
/// Each chapter ends where the next starts, in case a parent's length spans
/// its sections.
pub(crate) fn playable_chapters(
    info: &audible::models::ChapterInfo,
) -> Vec<audible::models::Chapter> {
    let mut chapters: Vec<_> = info
        .flatten()
        .into_iter()
        .map(|chapter| audible::models::Chapter {
            chapters: Vec::new(),
            ..chapter.clone()
        })
        .collect();
    chapters.sort_by_key(|chapter| chapter.start_offset_ms);
    for i in 1..chapters.len() {
        let next = chapters[i].start_offset_ms;
        let chapter = &mut chapters[i - 1];
        chapter.length_ms = chapter
            .length_ms
            .min(next.saturating_sub(chapter.start_offset_ms));
    }
    chapters.retain(|chapter| chapter.length_ms > 0);
    chapters
}

pub(crate) fn current_chapter(
    chapters: &[audible::models::Chapter],
    position_ms: u64,
) -> Option<(usize, &audible::models::Chapter)> {
    chapters.iter().enumerate().rev().find(|(_, chapter)| {
        chapter.length_ms > 0
            && position_ms >= chapter.start_offset_ms
            && position_ms < chapter.start_offset_ms.saturating_add(chapter.length_ms)
    })
}

/// Where playback sits on its timeline: the current chapter when chapters are
/// known, otherwise the whole book. Every player surface reads time and
/// scrubber position from this, so they agree.
#[derive(Clone, Debug)]
pub(crate) struct Timeline {
    /// Start of the span, in seconds into the book.
    pub start: f64,
    /// End of the span, in seconds into the book.
    pub end: f64,
    /// Playback position, in seconds into the book.
    pub position: f64,
    /// Book length in seconds.
    pub duration: f64,
    /// Zero-based chapter index and display name, when the span is a chapter.
    pub chapter: Option<(usize, String)>,
}

impl Timeline {
    /// The chapter containing `position`, or the whole book.
    pub(crate) fn chapter(
        chapters: &[audible::models::Chapter],
        position: f64,
        duration: f64,
    ) -> Self {
        let position = position.max(0.);
        let duration = duration.max(0.);
        match current_chapter(chapters, (position * 1000.) as u64) {
            Some((index, chapter)) => {
                let start = chapter.start_offset_ms as f64 / 1000.;
                let end = (chapter.start_offset_ms.saturating_add(chapter.length_ms) as f64
                    / 1000.)
                    .min(duration)
                    .max(start);
                let name = if chapter.title.is_empty() {
                    format!("Chapter {}", index + 1)
                } else {
                    chapter.title.clone()
                };
                Self {
                    start,
                    end,
                    position,
                    duration,
                    chapter: Some((index, name)),
                }
            }
            None => Self::book(position, duration),
        }
    }

    /// The whole book.
    pub(crate) fn book(position: f64, duration: f64) -> Self {
        let duration = duration.max(0.);
        Self {
            start: 0.,
            end: duration,
            position: position.clamp(0., duration),
            duration,
            chapter: None,
        }
    }

    fn length(&self) -> f64 {
        (self.end - self.start).max(0.)
    }

    /// Seconds played in the span.
    pub(crate) fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs((self.position - self.start).clamp(0., self.length()) as u64)
    }

    /// Seconds left in the span.
    pub(crate) fn remaining(&self) -> std::time::Duration {
        std::time::Duration::from_secs((self.end - self.position).clamp(0., self.length()) as u64)
    }

    /// Seconds left in the whole book.
    pub(crate) fn book_remaining(&self) -> std::time::Duration {
        std::time::Duration::from_secs((self.duration - self.position).max(0.) as u64)
    }

    /// Scrubber position within the span (0..=1).
    pub(crate) fn fraction(&self) -> f32 {
        let length = self.length();
        if length > 0. {
            ((self.position - self.start) / length).clamp(0., 1.) as f32
        } else {
            0.
        }
    }

    /// Converts a scrubber position in the span to a fraction of the book.
    pub(crate) fn book_fraction(&self, value: f32) -> f32 {
        if self.duration > 0. {
            ((self.start + self.length() * f64::from(value.clamp(0., 1.))) / self.duration) as f32
        } else {
            0.
        }
    }

    /// The chapter's name, if the span is a chapter.
    pub(crate) fn chapter_name(&self) -> Option<&str> {
        self.chapter.as_ref().map(|(_, name)| name.as_str())
    }
}

#[cfg(test)]
mod blurb_tests {
    use super::blurb_paragraphs;

    #[test]
    fn splits_paragraphs_and_drops_dividers() {
        let html = "<p>'A guide.' Charles Duhigg</p><p>'Refreshing.' Ali Abdaal</p>\
                    <p>________</p><p>Extraordinary results &amp; agency.</p>";
        assert_eq!(
            blurb_paragraphs(html),
            [
                "'A guide.' Charles Duhigg",
                "'Refreshing.' Ali Abdaal",
                "Extraordinary results & agency."
            ]
        );
    }

    #[test]
    fn splits_an_inline_divider() {
        assert_eq!(
            blurb_paragraphs("Praise here ________ Extraordinary results"),
            ["Praise here", "Extraordinary results"]
        );
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn a_chapter_with_sections_keeps_its_own_stretch() {
        // Designing Data-Intensive Applications: "Reliability" plays for three
        // minutes before its first section.
        let info: audible::models::ChapterInfo = serde_json::from_value(serde_json::json!({
            "chapters": [
                {"title": "Preface", "start_offset_ms": 0, "length_ms": 1_000},
                {"title": "Reliability", "start_offset_ms": 1_000, "length_ms": 500, "chapters": [
                    {"title": "Hardware Faults", "start_offset_ms": 1_500, "length_ms": 400},
                    {"title": "Software Errors", "start_offset_ms": 1_900, "length_ms": 300},
                ]},
                // A part whose length spans its sections.
                {"title": "Part II", "start_offset_ms": 2_200, "length_ms": 900, "chapters": [
                    {"title": "Replication", "start_offset_ms": 2_400, "length_ms": 700},
                ]},
            ],
        }))
        .unwrap();
        let chapters = playable_chapters(&info);
        let spans: Vec<_> = chapters
            .iter()
            .map(|c| (c.title.as_str(), c.start_offset_ms, c.length_ms))
            .collect();
        assert_eq!(
            spans,
            [
                ("Preface", 0, 1_000),
                ("Reliability", 1_000, 500),
                ("Hardware Faults", 1_500, 400),
                ("Software Errors", 1_900, 300),
                ("Part II", 2_200, 200),
                ("Replication", 2_400, 700),
            ]
        );
        // Every moment of the book is in a chapter.
        for ms in (0..3_100).step_by(50) {
            assert!(current_chapter(&chapters, ms).is_some(), "{ms} ms");
        }
        assert_eq!(current_chapter(&chapters, 1_200).unwrap().1.title, "Reliability");
    }

    fn item(runtime_min: u32, percent: f64, left: u64) -> audible::models::LibraryItem {
        serde_json::from_value(serde_json::json!({
            "asin": "B000000001",
            "runtime_length_min": runtime_min,
            "percent_complete": percent,
            "listening_status": {
                "percent_complete": percent,
                "time_remaining_seconds": left,
            },
        }))
        .unwrap()
    }

    #[test]
    fn a_local_place_sets_percent_and_time_left() {
        // Audible still says halfway through a 10-hour book.
        let mut book = item(600, 51., 5 * 3600);
        // Listening here reached 9 h 50 min of an exact 10-hour length.
        apply_position(&mut book, (9 * 60 + 50) * 60_000, Some(600 * 60_000));
        let percent = book_progress(&book).unwrap();
        assert!((percent - 98.333).abs() < 0.01, "{percent}");
        let status = book.listening_status.unwrap();
        assert_eq!(status.time_remaining_seconds, Some(10 * 60));
    }

    #[test]
    fn without_an_exact_length_the_runtime_is_used() {
        let mut book = item(120, 0., 7200);
        apply_position(&mut book, 30 * 60_000, None);
        assert_eq!(book_progress(&book), Some(25.));
        assert_eq!(
            book.listening_status.unwrap().time_remaining_seconds,
            Some(90 * 60)
        );
    }

    #[test]
    fn a_place_past_the_end_reads_as_finished_listening() {
        let mut book = item(60, 10., 3000);
        apply_position(&mut book, 2 * 3600 * 1000, Some(3600 * 1000));
        assert_eq!(book_progress(&book), Some(100.));
        assert_eq!(
            book.listening_status.unwrap().time_remaining_seconds,
            Some(0)
        );
    }
}
