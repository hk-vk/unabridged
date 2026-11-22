//! Read along: on-device transcripts of the playing chapter.
//!
//! Uses Apple's SpeechAnalyzer helper on macOS 26+, or a downloaded Whisper
//! model elsewhere. Finished chapters are cached per account and engine.

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
    /// The selected engine needs a one-time model download.
    NeedsModel,
    Downloading(f32),
    /// A streamed book: fetching this chapter's audio from Audible first.
    Fetching(f32),
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

/// Says when work for a job should stop: nobody shows it any more (the
/// listener moved to another chapter or book, so read along dropped it).
type Stop<'a> = &'a dyn Fn() -> bool;

/// The error a stopped job's work returns; it's never shown.
const STOPPED: &str = "Stopped";

/// Whether the read along holding `job` has let go of it, leaving only the
/// worker thread's reference.
fn abandoned(job: &SharedJob) -> bool {
    Arc::strong_count(job) <= 1
}

fn helper() -> Option<PathBuf> {
    let path = std::env::current_exe()
        .ok()?
        .parent()?
        .join("ba-transcribe");
    path.is_file().then_some(path)
}

const UNSUPPORTED: &str = "Read along needs macOS 26 or later.";

/// The on-device engine selected for a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// Apple's SpeechAnalyzer helper.
    Apple,
    /// Downloadable open Whisper model.
    Whisper,
}

/// Prefer Apple's installed helper, falling back only when unavailable or unsupported.
pub fn engine(locale: &str) -> Engine {
    // Testing: `UNABRIDGED_READ_ALONG_ENGINE=whisper` (or `apple`) picks the
    // engine on a Mac that has both.
    match std::env::var("UNABRIDGED_READ_ALONG_ENGINE").as_deref() {
        Ok("whisper") => return Engine::Whisper,
        Ok("apple") => return Engine::Apple,
        _ => {}
    }
    static ENGINES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, Engine>>> =
        std::sync::OnceLock::new();
    let engines = ENGINES.get_or_init(Mutex::default);
    if let Ok(engines) = engines.lock()
        && let Some(engine) = engines.get(locale)
    {
        return *engine;
    }
    if helper().is_none() {
        return Engine::Whisper;
    }
    let mut unsupported = false;
    let result = run(&["status", locale], &|| false, |value| {
        unsupported = value["status"].as_str() == Some("unsupported");
    });
    let engine = if unsupported || result.as_ref().is_err_and(|error| error == UNSUPPORTED) {
        Engine::Whisper
    } else {
        Engine::Apple
    };
    if let Ok(mut engines) = engines.lock() {
        engines.insert(locale.into(), engine);
    }
    engine
}

/// Runs the helper and hands each JSON line to `on_line`. A `{"error": …}`
/// line becomes the returned error.
fn run(
    args: &[&str],
    stop: Stop,
    mut on_line: impl FnMut(&serde_json::Value),
) -> Result<(), String> {
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
            if stop() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(STOPPED.into());
            }
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

/// The speech model locale for a book's Audible `language` ("english",
/// "mandarin_chinese", …), using the store (`marketplace`, its display name,
/// such as "United Kingdom") to pick the regional variant where the language
/// has several.
///
/// Covers every language Apple's on-device transcriber offers (macOS 26).
/// `Err` holds the message to show for any other language, rather than
/// running the book through a model for the wrong language. A book with no
/// language is assumed to be English.
pub fn locale_for(language: Option<&str>, marketplace: &str) -> Result<&'static str, String> {
    let language = language
        .map(|l| l.trim().to_ascii_lowercase().replace([' ', '-'], "_"))
        .filter(|l| !l.is_empty());
    let locale = match language.as_deref().unwrap_or("english") {
        "english" => match marketplace {
            "United Kingdom" => "en-GB",
            "Australia" => "en-AU",
            "Canada" => "en-CA",
            "India" => "en-IN",
            _ => "en-US",
        },
        "french" => match marketplace {
            "Canada" => "fr-CA",
            _ => "fr-FR",
        },
        "spanish" => match marketplace {
            "Spain" => "es-ES",
            "United States" => "es-US",
            _ => "es-MX",
        },
        // Audible's Portuguese catalogue is mostly Brazilian.
        "portuguese" | "brazilian_portuguese" => "pt-BR",
        "european_portuguese" => "pt-PT",
        "german" => "de-DE",
        "italian" => "it-IT",
        "japanese" => "ja-JP",
        "korean" => "ko-KR",
        "chinese" | "mandarin_chinese" | "simplified_chinese" => "zh-CN",
        "traditional_chinese" => "zh-TW",
        "cantonese" => "yue-CN",
        "hindi" => "hi-IN",
        "bengali" | "bangla" => "bn-IN",
        "gujarati" => "gu-IN",
        "kannada" => "kn-IN",
        "kashmiri" => "ks-IN",
        "maithili" => "mai-IN",
        "malayalam" => "ml-IN",
        "marathi" => "mr-IN",
        "nepali" => "ne-IN",
        "odia" | "oriya" => "or-IN",
        "punjabi" => "pa-IN",
        "tamil" => "ta-IN",
        "telugu" => "te-IN",
        "urdu" => "ur-IN",
        other => {
            return Err(format!(
                "Read along isn’t available for books in {} yet.",
                language_name(other)
            ));
        }
    };
    Ok(locale)
}

/// The name of a speech language from [`locale_for`], as shown when macOS
/// needs to add it: "English (US)", "Malayalam".
pub fn speech_name(locale: &str) -> &'static str {
    match locale {
        "en-US" => "English (US)",
        "en-GB" => "English (UK)",
        "en-AU" => "English (Australia)",
        "en-CA" => "English (Canada)",
        "en-IN" => "English (India)",
        "fr-FR" => "French",
        "fr-CA" => "French (Canada)",
        "es-ES" => "Spanish (Spain)",
        "es-US" => "Spanish (US)",
        "es-MX" => "Spanish (Mexico)",
        "pt-BR" => "Portuguese (Brazil)",
        "pt-PT" => "Portuguese (Portugal)",
        "de-DE" => "German",
        "it-IT" => "Italian",
        "ja-JP" => "Japanese",
        "ko-KR" => "Korean",
        "zh-CN" => "Chinese (Simplified)",
        "zh-TW" => "Chinese (Traditional)",
        "yue-CN" => "Cantonese",
        "hi-IN" => "Hindi",
        "bn-IN" => "Bengali",
        "gu-IN" => "Gujarati",
        "kn-IN" => "Kannada",
        "ks-IN" => "Kashmiri",
        "mai-IN" => "Maithili",
        "ml-IN" => "Malayalam",
        "mr-IN" => "Marathi",
        "ne-IN" => "Nepali",
        "or-IN" => "Odia",
        "pa-IN" => "Punjabi",
        "ta-IN" => "Tamil",
        "te-IN" => "Telugu",
        "ur-IN" => "Urdu",
        _ => "this language",
    }
}

/// "mandarin_chinese" → "Mandarin Chinese".
fn language_name(language: &str) -> String {
    language
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A read-along that can't run for this book, showing `message`.
pub fn unavailable(message: String) -> SharedJob {
    Arc::new(Mutex::new(Job {
        phase: Phase::Unsupported(message),
        lines: Vec::new(),
    }))
}

/// `Transcripts/<asin>/<locale>/<start-ms>.json`: one transcript per
/// language, so a chapter transcribed with the wrong language's model is
/// never shown once the right one is in use.
fn cache_path(key: &str, asin: &str, locale: &str, start_ms: u64) -> Option<PathBuf> {
    Some(
        transcripts_directory(key)?
            .join(match engine(locale) {
                Engine::Apple => "apple".into(),
                Engine::Whisper => format!(
                    "whisper-base-q8_0-{}",
                    crate::whisper::language(locale).ok()?
                ),
            })
            .join(asin)
            .join(locale)
            .join(format!("{start_ms}.json")),
    )
}

/// Before transcripts were kept per language they sat directly under the
/// book, and every one came from the English (US) model.
fn legacy_cache_path(key: &str, asin: &str, start_ms: u64) -> Option<PathBuf> {
    Some(
        transcripts_directory(key)?
            .join(asin)
            .join(format!("{start_ms}.json")),
    )
}

fn transcripts_directory(key: &str) -> Option<PathBuf> {
    Some(
        crate::backend::books_directory(key)?
            .parent()?
            .join("Transcripts"),
    )
}

fn cached(key: &str, asin: &str, locale: &str, start_ms: u64) -> Option<Vec<Line>> {
    let read = |path: PathBuf| -> Option<Vec<Line>> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    };
    read(cache_path(key, asin, locale, start_ms)?).or_else(|| {
        if engine(locale) == Engine::Apple {
            let path = transcripts_directory(key)?
                .join(asin)
                .join(locale)
                .join(format!("{start_ms}.json"));
            if let Some(lines) = read(path) {
                return Some(lines);
            }
        }
        (locale == "en-US" && engine(locale) == Engine::Apple)
            .then(|| legacy_cache_path(key, asin, start_ms))
            .flatten()
            .and_then(read)
    })
}

fn store(key: &str, asin: &str, locale: &str, start_ms: u64, lines: &[Line]) {
    if let Some(path) = cache_path(key, asin, locale, start_ms)
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
    /// For a streamed book, where to fetch `audio` from: it's the stream
    /// cache, which only holds what has been fetched.
    pub stream: Option<Arc<crate::streaming::Stream>>,
}

impl Span {
    fn start_ms(&self) -> u64 {
        (self.start * 1000.) as u64
    }
}

/// Makes sure a streamed span's audio for `from..to` is on this Mac,
/// reporting progress into `job`. Nothing to do for a download.
/// A long stretch is fetched a few minutes at a time, checking `stop`
/// between pieces.
fn fetch(
    span: &Span,
    from: f64,
    to: f64,
    job: Option<&SharedJob>,
    stop: Stop,
) -> Result<(), String> {
    let Some(stream) = &span.stream else {
        return Ok(());
    };
    let pieces = fetch_pieces(from, to);
    let count = pieces.len().max(1) as f32;
    for (index, (from, to)) in pieces.into_iter().enumerate() {
        if stop() {
            return Err(STOPPED.into());
        }
        let job = job.cloned();
        let progress = move |fraction: f32| {
            if let Some(job) = &job {
                set_phase(job, Phase::Fetching((index as f32 + fraction) / count));
            }
        };
        progress(0.);
        stream
            .prefetch(from, to, &progress)
            .map_err(|error| format!("Couldn’t get this chapter from Audible: {error}"))?;
    }
    Ok(())
}

/// `from..to` in stretches of at most [`STREAM_CHUNK`].
fn fetch_pieces(from: f64, to: f64) -> Vec<(f64, f64)> {
    let mut pieces = Vec::new();
    let mut cursor = from;
    while cursor < to {
        let end = (cursor + STREAM_CHUNK).min(to);
        pieces.push((cursor, end));
        cursor = end;
    }
    pieces
}

/// Transcribes `from..to` of the span's audio, handing each finished line
/// (ready to show) to `on_line`. Returns the lines as the model wrote them,
/// for the cache.
fn transcribe(
    span: &Span,
    from: f64,
    to: f64,
    stop: Stop,
    mut on_line: impl FnMut(Line),
) -> Result<Vec<Line>, String> {
    if engine(span.locale) == Engine::Whisper {
        return crate::whisper::transcribe(&span.audio, from, to, span.locale, stop, on_line)
            .map_err(|error| {
                if stop() {
                    STOPPED.into()
                } else {
                    error.to_string()
                }
            });
    }
    let mut lines = Vec::new();
    let audio = span.audio.to_string_lossy().to_string();
    let (start, end) = (format!("{from:.3}"), format!("{to:.3}"));
    run(
        &["transcribe", &audio, &start, &end, span.locale],
        stop,
        |value| {
            let line = Line {
                start: value["start"].as_f64().unwrap_or(0.),
                end: value["end"].as_f64().unwrap_or(0.),
                text: value["text"].as_str().unwrap_or_default().to_string(),
            };
            lines.push(line.clone());
            on_line(for_display(span.locale, line));
        },
    )?;
    Ok(lines)
}

/// How far before the playhead transcription starts, so the sentence being
/// spoken is caught whole.
const LEAD_IN: f64 = 15.;
/// First streamed stretch fetched and transcribed before the rest of the
/// chapter, so the visible lines around the playhead appear within seconds.
const STREAM_FIRST_WINDOW: f64 = 60.;
/// Follow-up streamed stretches fetched and transcribed in the background.
const STREAM_CHUNK: f64 = 180.;
/// A leftover tail shorter than this is folded into the previous stretch.
const STREAM_MIN_TAIL: f64 = 30.;

/// Where to start transcribing a chapter when listening from `playhead`:
/// just before it, or the chapter start when the playhead is near it.
fn split_point(span: &Span, playhead: f64) -> f64 {
    let split = (playhead - LEAD_IN).clamp(span.start, span.end);
    if split - span.start < LEAD_IN * 2. {
        span.start
    } else {
        split
    }
}

/// Splits a streamed range `from..to` into a short first stretch (for fast
/// first-page visibility) followed by background stretches for the rest.
fn stream_windows(from: f64, to: f64) -> Vec<(f64, f64)> {
    if !from.is_finite() || !to.is_finite() || to <= from {
        return Vec::new();
    }
    if to - from <= STREAM_FIRST_WINDOW + STREAM_MIN_TAIL {
        return vec![(from, to)];
    }
    let mut windows = Vec::new();
    let first_end = from + STREAM_FIRST_WINDOW;
    windows.push((from, first_end));
    let mut cursor = first_end;
    while cursor < to {
        if to - cursor <= STREAM_CHUNK + STREAM_MIN_TAIL {
            windows.push((cursor, to));
            break;
        }
        let end = cursor + STREAM_CHUNK;
        windows.push((cursor, end));
        cursor = end;
    }
    windows
}

/// The cache keeps what the model wrote; this is what read along shows.
/// Apple's Malayalam model writes Manglish, so it goes back into Malayalam
/// script here (and improving the conversion never needs a re-transcribe).
fn for_display(locale: &str, mut line: Line) -> Line {
    if locale == "ml-IN" && engine(locale) == Engine::Apple {
        line.text = crate::malayalam::from_manglish(&line.text);
    }
    line
}

/// Starts reading along `span`: from the cache, or by checking the model and
/// transcribing on a worker thread. `next` is transcribed afterwards into the
/// cache so the following chapter is ready when playback gets there.
///
/// A chapter is transcribed from just before `playhead` to its end first, so
/// the lines being listened to appear within seconds even deep into a long
/// chapter; the part before the playhead follows and is added in one go,
/// so the list moves once instead of with every line.
pub fn start(span: Span, next: Option<Span>, playhead: f64) -> SharedJob {
    if let Some(lines) = cached(&span.key, &span.asin, span.locale, span.start_ms()) {
        let job = Arc::new(Mutex::new(Job {
            phase: Phase::Ready,
            lines: lines
                .into_iter()
                .map(|line| for_display(span.locale, line))
                .collect(),
        }));
        if let Some(next) = next {
            let watch = Arc::downgrade(&job);
            std::thread::spawn(move || prefetch(next, &|| watch.strong_count() == 0));
        }
        return job;
    }
    let job = Arc::new(Mutex::new(Job {
        phase: Phase::Checking,
        lines: Vec::new(),
    }));
    let worker = job.clone();
    std::thread::spawn(move || {
        let stop = || abandoned(&worker);
        match model_status(span.locale) {
            Ok(true) => {}
            Ok(false) => return set_phase(&worker, Phase::NeedsModel),
            Err(message) => return set_phase(&worker, Phase::Unsupported(message)),
        }
        set_phase(&worker, Phase::Transcribing);
        let split = split_point(&span, playhead);
        let mut push = |line: Line| {
            if let Ok(mut job) = worker.lock() {
                job.lines.push(line);
            }
        };
        let ahead_result = if span.stream.is_some() {
            let mut ahead = Vec::new();
            let mut err = None;
            for (index, (from, to)) in stream_windows(split, span.end).into_iter().enumerate() {
                // Only show Fetching on the first short stretch while no lines
                // are visible yet; later chunks fetch silently in the
                // background while the user reads along.
                let progress_job = (index == 0).then_some(&worker);
                if let Err(message) = fetch(&span, from, to, progress_job, &stop) {
                    err = Some(message);
                    break;
                }
                set_phase(&worker, Phase::Transcribing);
                match transcribe(&span, from, to, &stop, &mut push) {
                    Ok(lines) => ahead.extend(lines),
                    Err(message) => {
                        err = Some(message);
                        break;
                    }
                }
            }
            match err {
                Some(message) => Err(message),
                None => Ok(ahead),
            }
        } else {
            transcribe(&span, split, span.end, &stop, &mut push)
        };
        let result = ahead_result.and_then(|ahead| {
            if split <= span.start {
                return Ok(ahead);
            }
            fetch(&span, span.start, split, None, &stop)?;
            let mut before = transcribe(&span, span.start, split, &stop, |_| {})?;
            if let Ok(mut job) = worker.lock() {
                let shown: Vec<Line> = before
                    .iter()
                    .cloned()
                    .map(|line| for_display(span.locale, line))
                    .collect();
                job.lines.splice(0..0, shown);
            }
            before.extend(ahead);
            Ok(before)
        });
        match result {
            Ok(lines) => {
                store(&span.key, &span.asin, span.locale, span.start_ms(), &lines);
                set_phase(&worker, Phase::Ready);
            }
            // Nobody is watching a stopped job.
            Err(message) if message == STOPPED => return,
            Err(message) => set_phase(&worker, Phase::Failed(message)),
        }
        if let Some(next) = next {
            prefetch(next, &stop);
        }
    });
    job
}

fn prefetch(span: Span, stop: Stop) {
    if cached(&span.key, &span.asin, span.locale, span.start_ms()).is_none()
        && model_status(span.locale) == Ok(true)
        && fetch(&span, span.start, span.end, None, stop).is_ok()
        && let Ok(lines) = if engine(span.locale) == Engine::Whisper {
            crate::whisper::prefetch(&span.audio, span.start, span.end, span.locale, stop)
                .map_err(|error| error.to_string())
        } else {
            transcribe(&span, span.start, span.end, stop, |_| {})
        }
    {
        store(&span.key, &span.asin, span.locale, span.start_ms(), &lines);
    }
}

/// Whether the speech model for `locale` is installed. `Err` when this Mac
/// cannot transcribe the language at all.
fn model_status(locale: &str) -> Result<bool, String> {
    if engine(locale) == Engine::Whisper {
        crate::whisper::language(locale).map_err(|error| error.to_string())?;
        return crate::whisper::status()
            .map(|status| status == crate::whisper::Status::Ready)
            .map_err(|error| error.to_string());
    }
    let mut status = None;
    run(&["status", locale], &|| false, |value| {
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
        let result = if engine(locale) == Engine::Whisper {
            crate::whisper::download(|progress| set_phase(&job, Phase::Downloading(progress)))
                .map_err(|error| error.to_string())
        } else {
            run(&["install", locale], &|| false, |value| {
                if let Some(progress) = value["progress"].as_f64() {
                    set_phase(&job, Phase::Downloading(progress as f32));
                }
            })
        };
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
    fn stream_windows_prioritize_the_first_page_then_chunk_the_rest() {
        assert!(stream_windows(10., 10.).is_empty());
        assert_eq!(stream_windows(0., 45.), vec![(0., 45.)]);
        assert_eq!(stream_windows(0., 85.), vec![(0., 85.)]);
        assert_eq!(
            stream_windows(0., 600.),
            vec![(0., 60.), (60., 240.), (240., 420.), (420., 600.)]
        );
        // A tiny leftover tail folds into the previous stretch.
        assert_eq!(stream_windows(0., 255.), vec![(0., 60.), (60., 255.)]);
        // Mid-chapter start keeps the first window tight around the playhead.
        assert_eq!(
            stream_windows(285., 600.),
            vec![(285., 345.), (345., 525.), (525., 600.)]
        );
    }

    #[test]
    fn long_fetches_go_in_pieces_so_they_can_stop() {
        assert_eq!(
            fetch_pieces(0., 400.),
            vec![(0., 180.), (180., 360.), (360., 400.)]
        );
        assert!(fetch_pieces(5., 5.).is_empty());
    }

    #[test]
    fn a_job_is_abandoned_once_read_along_lets_go() {
        let job: SharedJob = Arc::new(Mutex::new(Job {
            phase: Phase::Checking,
            lines: Vec::new(),
        }));
        let worker = job.clone();
        assert!(!abandoned(&worker));
        drop(job);
        assert!(abandoned(&worker));
    }

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
    }

    #[test]
    fn languages_map_to_the_right_model() {
        let us = "United States";
        assert_eq!(locale_for(Some("German"), us), Ok("de-DE"));
        assert_eq!(locale_for(None, us), Ok("en-US"));
        assert_eq!(locale_for(Some(""), us), Ok("en-US"));
        // The store picks the regional variant.
        assert_eq!(locale_for(Some("english"), "United Kingdom"), Ok("en-GB"));
        assert_eq!(locale_for(None, "India"), Ok("en-IN"));
        assert_eq!(locale_for(Some("english"), "Australia"), Ok("en-AU"));
        assert_eq!(locale_for(Some("french"), "Canada"), Ok("fr-CA"));
        assert_eq!(locale_for(Some("french"), "France"), Ok("fr-FR"));
        assert_eq!(locale_for(Some("spanish"), "Spain"), Ok("es-ES"));
        assert_eq!(locale_for(Some("spanish"), us), Ok("es-US"));
        assert_eq!(locale_for(Some("portuguese"), "Brazil"), Ok("pt-BR"));
        // Languages beyond the original ten.
        assert_eq!(locale_for(Some("tamil"), "India"), Ok("ta-IN"));
        assert_eq!(locale_for(Some("Bengali"), "India"), Ok("bn-IN"));
        assert_eq!(locale_for(Some("cantonese"), us), Ok("yue-CN"));
        assert_eq!(locale_for(Some("mandarin_chinese"), us), Ok("zh-CN"));
        // Anything else says so instead of falling back to English.
        assert_eq!(
            locale_for(Some("dutch"), us),
            Err("Read along isn’t available for books in Dutch yet.".into())
        );
        assert_eq!(
            locale_for(Some("brazilian_sign_language"), us),
            Err("Read along isn’t available for books in Brazilian Sign Language yet.".into())
        );
    }

    /// Every locale we map to must be one Apple's transcriber offers
    /// (`SpeechTranscriber.supportedLocales` on macOS 26).
    #[test]
    fn mapped_locales_are_ones_apple_supports() {
        const APPLE: &[&str] = &[
            "bn-IN", "de-AT", "de-CH", "de-DE", "en-AU", "en-CA", "en-GB", "en-IE", "en-IN",
            "en-NZ", "en-SG", "en-US", "en-ZA", "es-CL", "es-ES", "es-MX", "es-US", "fr-BE",
            "fr-CA", "fr-CH", "fr-FR", "gu-IN", "hi-IN", "it-CH", "it-IT", "ja-JP", "kn-IN",
            "ko-KR", "ks-IN", "mai-IN", "ml-IN", "mr-IN", "ne-IN", "or-IN", "pa-IN", "pt-BR",
            "pt-PT", "ta-IN", "te-IN", "ur-IN", "yue-CN", "zh-CN", "zh-HK", "zh-TW",
        ];
        let languages = [
            "english",
            "french",
            "spanish",
            "portuguese",
            "brazilian_portuguese",
            "european_portuguese",
            "german",
            "italian",
            "japanese",
            "korean",
            "chinese",
            "mandarin_chinese",
            "simplified_chinese",
            "traditional_chinese",
            "cantonese",
            "hindi",
            "bengali",
            "bangla",
            "gujarati",
            "kannada",
            "kashmiri",
            "maithili",
            "malayalam",
            "marathi",
            "nepali",
            "odia",
            "oriya",
            "punjabi",
            "tamil",
            "telugu",
            "urdu",
        ];
        for marketplace in audible::Marketplace::ALL {
            for language in languages {
                let locale = locale_for(Some(language), marketplace.display_name()).unwrap();
                assert!(APPLE.contains(&locale), "{language} → {locale}");
            }
        }
    }
}
