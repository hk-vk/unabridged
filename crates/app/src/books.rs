//! Pure helpers over library data: progress, chapters, download stages, blurbs.

/// Plain text from Audible's HTML blurbs: tags dropped, entities decoded,
/// whitespace collapsed.
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
            ["'A guide.' Charles Duhigg", "'Refreshing.' Ali Abdaal", "Extraordinary results & agency."]
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
