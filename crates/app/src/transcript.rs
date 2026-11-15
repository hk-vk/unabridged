//! Read along: on-device transcripts of the playing chapter.
//!
//! Transcription runs in the `ba-transcribe` helper (Apple's SpeechAnalyzer,
//! macOS 26+), which sits next to the app binary. Finished chapters are cached
//! per account under `Transcripts/<asin>/<start-ms>.json`, so each chapter is
//! transcribed once.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// One recognised sentence, with absolute book times in seconds.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Line {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Where a chapter's read-along stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Checking,
    /// The speech model for this language must be downloaded first.
    NeedsModel,
    Downloading(f32),
    Transcribing,
    Ready,
    Failed(String),
    /// This Mac cannot transcribe (no helper, or macOS before 26).
    Unsupported(String),
}

/// A chapter's transcript in progress, shared with the worker thread.
pub struct Job {
    pub phase: Phase,
    pub lines: Vec<Line>,
}

pub type SharedJob = Arc<Mutex<Job>>;

fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe()
        .ok()?
        .parent()?
        .join("ba-transcribe");
    path.is_file().then_some(path)
}

const UNSUPPORTED: &str = "Read along needs macOS 26 or later.";

/// Runs the helper and hands each JSON line to `on_line`. A `{"error": …}`
/// line becomes the returned error.
fn run(args: &[&str], mut on_line: impl FnMut(&serde_json::Value)) -> Result<(), String> {
    let helper = helper().ok_or_else(|| UNSUPPORTED.to_string())?;
    let mut child = Command::new(helper)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| UNSUPPORTED.to_string())?;
    let mut error = None;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if let Some(message) = value.get("error").and_then(|e| e.as_str()) {
                error = Some(message.to_string());
            } else {
                on_line(&value);
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    match error {
        Some(message) => Err(message),
        // Killed by dyld on an older macOS, for example.
        None if !status.success() => Err(UNSUPPORTED.into()),
        None => Ok(()),
    }
}

/// The helper's locale for a book's `language` ("english", "german", …).
pub fn locale_for(language: Option<&str>) -> &'static str {
    match language.map(|l| l.to_ascii_lowercase()).as_deref() {
        Some("german") => "de-DE",
        Some("french") => "fr-FR",
        Some("spanish") => "es-ES",
        Some("italian") => "it-IT",
        Some("portuguese") => "pt-BR",
        Some("japanese") => "ja-JP",
        Some("chinese" | "mandarin_chinese") => "zh-CN",
        Some("korean") => "ko-KR",
        Some("hindi") => "hi-IN",
        _ => "en-US",
    }
}

fn cache_path(key: &str, asin: &str, start_ms: u64) -> Option<PathBuf> {
    let books = crate::backend::books_directory(key)?;
    Some(
        books
            .parent()?
            .join("Transcripts")
            .join(asin)
            .join(format!("{start_ms}.json")),
    )
}

fn cached(key: &str, asin: &str, start_ms: u64) -> Option<Vec<Line>> {
    let bytes = std::fs::read(cache_path(key, asin, start_ms)?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn store(key: &str, asin: &str, start_ms: u64, lines: &[Line]) {
    if let Some(path) = cache_path(key, asin, start_ms)
        && let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
        && let Ok(bytes) = serde_json::to_vec(lines)
    {
        let _ = std::fs::write(path, bytes);
    }
}

fn set_phase(job: &SharedJob, phase: Phase) {
    if let Ok(mut job) = job.lock() {
        job.phase = phase;
    }
}

/// A span of audio to transcribe, in seconds.
#[derive(Clone)]
pub struct Span {
    pub key: String,
    pub asin: String,
    pub audio: PathBuf,
    pub start: f64,
    pub end: f64,
    pub locale: &'static str,
}

impl Span {
    fn start_ms(&self) -> u64 {
        (self.start * 1000.) as u64
    }
}

fn transcribe(span: &Span, mut on_line: impl FnMut(Line)) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    let audio = span.audio.to_string_lossy().to_string();
    let (start, end) = (format!("{:.3}", span.start), format!("{:.3}", span.end));
    run(
        &["transcribe", &audio, &start, &end, span.locale],
        |value| {
            let line = Line {
                start: value["start"].as_f64().unwrap_or(0.),
                end: value["end"].as_f64().unwrap_or(0.),
                text: value["text"].as_str().unwrap_or_default().to_string(),
            };
            lines.push(line.clone());
            on_line(line);
        },
    )?;
    store(&span.key, &span.asin, span.start_ms(), &lines);
    Ok(lines)
}

/// Starts reading along `span`: from the cache, or by checking the model and
/// transcribing on a worker thread. `next` is transcribed afterwards into the
/// cache so the following chapter is ready when playback gets there.
pub fn start(span: Span, next: Option<Span>) -> SharedJob {
    if let Some(lines) = cached(&span.key, &span.asin, span.start_ms()) {
        let job = Arc::new(Mutex::new(Job {
            phase: Phase::Ready,
            lines,
        }));
        if let Some(next) = next {
            std::thread::spawn(move || prefetch(next));
        }
        return job;
    }
    let job = Arc::new(Mutex::new(Job {
        phase: Phase::Checking,
        lines: Vec::new(),
    }));
    let worker = job.clone();
    std::thread::spawn(move || {
        match model_status(span.locale) {
            Ok(true) => {}
            Ok(false) => return set_phase(&worker, Phase::NeedsModel),
            Err(message) => return set_phase(&worker, Phase::Unsupported(message)),
        }
        set_phase(&worker, Phase::Transcribing);
        let result = transcribe(&span, |line| {
            if let Ok(mut job) = worker.lock() {
                job.lines.push(line);
            }
        });
        match result {
            Ok(_) => set_phase(&worker, Phase::Ready),
            Err(message) => set_phase(&worker, Phase::Failed(message)),
        }
        if let Some(next) = next {
            prefetch(next);
        }
    });
    job
}

fn prefetch(span: Span) {
    if cached(&span.key, &span.asin, span.start_ms()).is_none()
        && model_status(span.locale) == Ok(true)
    {
        let _ = transcribe(&span, |_| {});
    }
}

/// Whether the speech model for `locale` is installed. `Err` when this Mac
/// cannot transcribe the language at all.
fn model_status(locale: &str) -> Result<bool, String> {
    let mut status = None;
    run(&["status", locale], |value| {
        status = value["status"].as_str().map(str::to_string);
    })?;
    match status.as_deref() {
        Some("installed") => Ok(true),
        Some("supported" | "downloading") => Ok(false),
        _ => Err("Read along does not support this book's language yet.".into()),
    }
}

/// Downloads the speech model, reporting progress into `job`. The caller
/// restarts the read-along once the phase leaves `Downloading`.
pub fn install_model(locale: &'static str, job: SharedJob) {
    set_phase(&job, Phase::Downloading(0.));
    std::thread::spawn(move || {
        let result = run(&["install", locale], |value| {
            if let Some(progress) = value["progress"].as_f64() {
                set_phase(&job, Phase::Downloading(progress as f32));
            }
        });
        set_phase(
            &job,
            match result {
                Ok(()) => Phase::Checking,
                Err(message) => Phase::Failed(message),
            },
        );
    });
}

/// The index of the line being spoken at `position`, or the last one before it.
pub fn current_line(lines: &[Line], position: f64) -> Option<usize> {
    let index = lines.partition_point(|line| line.start <= position);
    index.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_line_tracks_the_playhead() {
        let line = |start, end| Line {
            start,
            end,
            text: String::new(),
        };
        let lines = [line(10., 12.), line(12.5, 15.), line(16., 20.)];
        assert_eq!(current_line(&lines, 5.), None);
        assert_eq!(current_line(&lines, 10.), Some(0));
        assert_eq!(current_line(&lines, 12.2), Some(0));
        assert_eq!(current_line(&lines, 13.), Some(1));
        assert_eq!(current_line(&lines, 99.), Some(2));
        assert_eq!(locale_for(Some("German")), "de-DE");
        assert_eq!(locale_for(None), "en-US");
    }
}
