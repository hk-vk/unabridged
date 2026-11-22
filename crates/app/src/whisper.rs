//! Downloadable, local Whisper transcription for machines without Apple SpeechAnalyzer.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error,
    formats::{FormatOptions, SeekMode, SeekTo, Track},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::transcript::Line;

/// The multilingual whisper.cpp models read along can download. Only the
/// chosen one is kept on disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Model {
    /// large-v3-turbo, five-bit: near large-v3 accuracy, and much better than
    /// the small models with accents and names.
    #[default]
    Turbo,
    /// small, eight-bit: less than half the download, faster on older
    /// computers.
    Small,
}

impl Model {
    pub const ALL: [Self; 2] = [Self::Turbo, Self::Small];

    pub fn label(self) -> &'static str {
        match self {
            Self::Turbo => "Most accurate",
            Self::Small => "Smaller",
        }
    }

    /// Names the model in transcript cache paths, so one model's
    /// transcripts never stand in for another's.
    pub fn id(self) -> &'static str {
        match self {
            Self::Turbo => "large-v3-turbo-q5_0",
            Self::Small => "small-q8_0",
        }
    }

    fn file(self) -> String {
        format!("ggml-{}.bin", self.id())
    }

    fn url(self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.file()
        )
    }

    /// Verified Hugging Face LFS object size in bytes.
    fn size(self) -> u64 {
        match self {
            Self::Turbo => 574_041_195,
            Self::Small => 264_464_607,
        }
    }

    /// SHA-256 of the model payload, not its LFS pointer.
    fn sha256(self) -> &'static str {
        match self {
            Self::Turbo => "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
            Self::Small => "49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f",
        }
    }

    /// Rounded-up decimal megabytes for download labels.
    pub fn megabytes(self) -> u64 {
        self.size().div_ceil(1_000_000)
    }
}

/// The model in use, from Settings.
static SELECTED: AtomicU8 = AtomicU8::new(0);

pub fn selected() -> Model {
    Model::ALL
        .get(SELECTED.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}

/// Uses `model` from now on. Blocking: deletes the other models' files
/// (and the base model earlier versions downloaded) so only one is on disk.
pub fn select(model: Model) {
    let index = Model::ALL.iter().position(|m| *m == model).unwrap_or(0);
    SELECTED.store(index as u8, Ordering::Relaxed);
    let Ok(_guard) = MODEL_IO.lock() else {
        return;
    };
    let Ok(directory) = models_directory() else {
        return;
    };
    let others = Model::ALL
        .into_iter()
        .filter(|other| *other != model)
        .map(Model::file)
        .chain(["ggml-base-q8_0.bin".to_string()]);
    for file in others {
        let _ = std::fs::remove_file(directory.join(&file));
        let _ = std::fs::remove_file(directory.join(format!("{file}.part")));
    }
}

static CONTEXT: Mutex<Option<(Model, WhisperContext)>> = Mutex::new(None);
static MODEL_IO: Mutex<()> = Mutex::new(());
static FOREGROUND_REQUESTS: AtomicUsize = AtomicUsize::new(0);

struct Request<'a> {
    foreground: &'a AtomicUsize,
    prefetch: bool,
    /// Nobody wants the result any more (read along moved on): stop at the
    /// next window so a newer request gets the engine.
    stop: &'a dyn Fn() -> bool,
}

impl<'a> Request<'a> {
    fn new(foreground: &'a AtomicUsize, prefetch: bool, stop: &'a dyn Fn() -> bool) -> Self {
        if !prefetch {
            foreground.fetch_add(1, Ordering::SeqCst);
        }
        Self {
            foreground,
            prefetch,
            stop,
        }
    }

    fn check(&self) -> Result<()> {
        ensure!(!(self.stop)(), "Transcription stopped");
        ensure!(
            !self.prefetch || self.foreground.load(Ordering::SeqCst) == 0,
            "Speech prefetch interrupted by foreground transcription"
        );
        Ok(())
    }

    fn lock<'b, T>(&self, context: &'b Mutex<T>) -> Result<MutexGuard<'b, T>> {
        self.check()?;
        let guard = if self.prefetch {
            context
                .try_lock()
                .map_err(|_| anyhow::anyhow!("Speech engine busy"))?
        } else {
            context
                .lock()
                .map_err(|_| anyhow::anyhow!("Speech engine lock failed"))?
        };
        self.check()?;
        Ok(guard)
    }
}

impl Drop for Request<'_> {
    fn drop(&mut self) {
        if !self.prefetch {
            self.foreground.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Whether the optional local model is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No complete model is installed.
    NotDownloaded,
    /// A complete model is available locally.
    Ready,
}

fn models_directory() -> Result<PathBuf> {
    Ok(crate::paths::support()?.join("Models"))
}

fn model_path(model: Model) -> Result<PathBuf> {
    Ok(models_directory()?.join(model.file()))
}

/// Inspect the downloaded model without loading it into memory.
pub fn status() -> Result<Status> {
    Ok(
        if model_path(selected())?
            .metadata()
            .is_ok_and(|m| m.len() == selected().size())
        {
            Status::Ready
        } else {
            Status::NotDownloaded
        },
    )
}

/// Download and verify the model, reporting progress from zero to one.
pub fn download(mut progress: impl FnMut(f32)) -> Result<()> {
    let _guard = MODEL_IO
        .lock()
        .map_err(|_| anyhow::anyhow!("Model download lock failed"))?;
    if status()? == Status::Ready {
        progress(1.);
        return Ok(());
    }
    let model = selected();
    let path = model_path(model)?;
    std::fs::create_dir_all(path.parent().context("No models directory")?)?;
    let part = path.with_extension("bin.part");
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut response = reqwest::Client::new()
                .get(model.url())
                .send()
                .await?
                .error_for_status()?;
            let mut file = std::fs::File::create(&part)?;
            let mut hash = Sha256::new();
            let mut received = 0u64;
            while let Some(chunk) = response.chunk().await? {
                received += chunk.len() as u64;
                ensure!(
                    received <= model.size(),
                    "Speech model exceeds expected size"
                );
                hash.update(&chunk);
                file.write_all(&chunk)?;
                progress(received as f32 / model.size() as f32);
            }
            ensure!(received == model.size(), "Incomplete speech model download");
            ensure!(
                hex::encode(hash.finalize()) == model.sha256(),
                "Speech model checksum mismatch"
            );
            file.sync_all()?;
            drop(file);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            std::fs::rename(&part, &path)?;
            Ok(())
        });
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}

/// Remove the downloaded model and release its in-memory context.
pub fn remove() -> Result<()> {
    let _guard = MODEL_IO
        .lock()
        .map_err(|_| anyhow::anyhow!("Model download lock failed"))?;
    let mut context = CONTEXT
        .lock()
        .map_err(|_| anyhow::anyhow!("Speech engine lock failed"))?;
    *context = None;
    match std::fs::remove_file(model_path(selected())?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Convert a BCP-47 locale to a language supported by Whisper.
/// Languages the base model transcribes well enough to read along with.
/// Others come out wrong, often in another script (Malayalam came out as
/// Tamil and Urdu script), so read along says it isn't available instead.
const SUPPORTED: &[&str] = &[
    "en", "es", "fr", "de", "it", "pt", "nl", "ru", "pl", "uk", "ca", "sv", "ja", "zh", "ko", "tr",
];

pub fn language(locale: &str) -> Result<String> {
    let mut code = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if code == "yue" {
        code = "zh".into();
    }
    ensure!(
        SUPPORTED.contains(&code.as_str())
            && whisper_rs::get_lang_id(&code).is_some_and(|id| id < 99),
        "Read along isn’t available for this book’s language on this computer yet."
    );
    Ok(code)
}

// A windowed sinc low-pass filter avoids aliasing when reducing audiobook sample rates.
fn resample(input: &[f32], rate: u32) -> Vec<f32> {
    if rate == 16_000 {
        return input.to_vec();
    }
    let count = (input.len() as u64 * 16_000 / u64::from(rate)) as usize;
    let cutoff = (16_000. / f64::from(rate)).min(1.) * 0.95;
    (0..count)
        .map(|i| {
            let position = i as f64 * f64::from(rate) / 16_000.;
            let center = position.floor() as isize;
            let mut sum = 0.;
            let mut weight = 0.;
            for index in center - 24..=center + 24 {
                if index < 0 || index as usize >= input.len() {
                    continue;
                }
                let distance = position - index as f64;
                let x = std::f64::consts::PI * distance * cutoff;
                let sinc = if x.abs() < 1e-10 { 1. } else { x.sin() / x };
                let w = sinc * (0.5 + 0.5 * (std::f64::consts::PI * distance / 25.).cos());
                sum += f64::from(input[index as usize]) * w;
                weight += w;
            }
            (sum / weight) as f32
        })
        .collect()
}

fn absolute_line(offset: f64, start: i64, end: i64, limit: f64, text: String) -> Line {
    Line {
        start: (offset + start as f64 * 0.01).min(limit),
        end: (offset + end as f64 * 0.01).min(limit),
        text,
    }
}

fn audio_track(tracks: &[Track]) -> Result<&Track> {
    tracks
        .iter()
        .find(|track| {
            track.codec_params.codec != CODEC_TYPE_NULL
                && track.codec_params.sample_rate.is_some_and(|rate| rate > 0)
        })
        .context("No audio track")
}

/// Decode and transcribe a range, streaming absolute-time segments after each window.
pub fn transcribe(
    path: &Path,
    from: f64,
    to: f64,
    locale: &str,
    stop: &dyn Fn() -> bool,
    on_line: impl FnMut(Line),
) -> Result<Vec<Line>> {
    let request = Request::new(&FOREGROUND_REQUESTS, false, stop);
    transcribe_with_request(path, from, to, locale, on_line, &request)
}

/// Prefetch only while the engine is idle, yielding between windows to live read along.
pub fn prefetch(
    path: &Path,
    from: f64,
    to: f64,
    locale: &str,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<Line>> {
    let request = Request::new(&FOREGROUND_REQUESTS, true, stop);
    transcribe_with_request(path, from, to, locale, |_| {}, &request)
}

fn transcribe_with_request(
    path: &Path,
    from: f64,
    to: f64,
    locale: &str,
    mut on_line: impl FnMut(Line),
    request: &Request<'_>,
) -> Result<Vec<Line>> {
    request.check()?;
    ensure!(
        from.is_finite() && to.is_finite() && from >= 0. && to > from,
        "Invalid transcription range"
    );
    let language = language(locale)?;
    let file = std::fs::File::open(path)?;
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?
        .format;
    let track = audio_track(format.tracks())?;
    let track_id = track.id;
    // HE-AAC files give the full (SBR) rate here, but Symphonia decodes
    // only the AAC core at half of it, so the decoded rate is what counts.
    let mut rate = track
        .codec_params
        .sample_rate
        .context("No audio sample rate")?;
    ensure!(rate > 0, "Invalid audio sample rate");
    let mut rate_known = false;
    let time_base = track.codec_params.time_base.context("No audio time base")?;
    let mut decoder = symphonia::default::get_codecs().make(
        &decoder_params(&track.codec_params),
        &DecoderOptions::default(),
    )?;
    format.seek(
        SeekMode::Accurate,
        SeekTo::Time {
            time: Time::from(from),
            track_id: Some(track_id),
        },
    )?;
    decoder.reset();
    let mut context = request.lock(&CONTEXT)?;
    let model = selected();
    if context.as_ref().is_none_or(|(loaded, _)| *loaded != model) {
        // Free the old model before loading the new one.
        *context = None;
        let path = model_path(model)?;
        *context = Some((
            model,
            WhisperContext::new_with_params(
                path.to_str().context("Invalid model path")?,
                WhisperContextParameters::default(),
            )?,
        ));
    }
    let mut state = context
        .as_ref()
        .context("Speech engine unavailable")?
        .1
        .create_state()?;
    let threads =
        std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1)) as i32;
    let mut lines = Vec::new();
    let mut samples = Vec::new();
    let mut offset = from;
    let mut infer = |audio: &[f32], offset: f64, rate: u32| -> Result<()> {
        request.check()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&language));
        params.set_n_threads(threads);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_no_context(true);
        state.full(params, &resample(audio, rate))?;
        for segment in state.as_iter() {
            let line = absolute_line(
                offset,
                segment.start_timestamp(),
                segment.end_timestamp(),
                to,
                segment.to_str_lossy()?.trim().to_string(),
            );
            if line.start < line.end && !line.text.is_empty() {
                on_line(line.clone());
                lines.push(line);
            }
        }
        Ok(())
    };
    loop {
        request.check()?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let time = time_base.calc_time(packet.ts());
        let packet_start = time.seconds as f64 + time.frac;
        if packet_start >= to {
            break;
        }
        // A damaged packet costs a few milliseconds of audio, not the chapter.
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(Error::DecodeError(_) | Error::IoError(_)) => continue,
            Err(error) => return Err(error.into()),
        };
        let channels = decoded.spec().channels.count();
        if !rate_known {
            rate = decoded.spec().rate;
            rate_known = true;
        }
        ensure!(
            channels > 0 && rate > 0 && decoded.spec().rate == rate,
            "Audio format changed during transcription"
        );
        let window = rate as usize * 30;
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        for (index, frame) in buffer.samples().chunks(channels).enumerate() {
            let time = packet_start + index as f64 / f64::from(rate);
            if time >= from && time < to {
                samples.push(frame.iter().sum::<f32>() / channels as f32);
            }
        }
        while samples.len() >= window {
            infer(&samples[..window], offset, rate)?;
            samples.drain(..window);
            offset += 30.;
        }
    }
    if !samples.is_empty() {
        infer(&samples, offset, rate)?;
    }
    Ok(lines)
}

/// Some Audible AAC-LC files set "depends on core coder" in their two-byte
/// AudioSpecificConfig without the delay that should follow. Apple's decoder
/// ignores it; Symphonia stops with "unexpected end of bitstream". The flag
/// means nothing for AAC-LC, so it's cleared.
fn decoder_params(
    params: &symphonia::core::codecs::CodecParameters,
) -> symphonia::core::codecs::CodecParameters {
    let mut params = params.clone();
    if let Some(config) = params.extra_data.as_mut()
        && config.len() == 2
        && config[0] >> 3 == 2
    {
        config[1] &= !0b111;
    }
    params
}

/// Frees the loaded model before the process exits. ggml's Metal backend
/// asserts at exit while a model is still loaded, which crashed the app on
/// quit after read along had run. Skipped if a transcription holds it.
pub fn shutdown() {
    if let Ok(mut context) = CONTEXT.try_lock() {
        context.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_model_has_its_own_file_and_cache_name() {
        assert_eq!(Model::Turbo.file(), "ggml-large-v3-turbo-q5_0.bin");
        assert_eq!(Model::Small.file(), "ggml-small-q8_0.bin");
        assert_eq!(Model::Turbo.megabytes(), 575);
        assert_eq!(Model::Small.megabytes(), 265);
        assert_eq!(selected(), Model::Turbo);
    }

    #[test]
    fn a_stopped_request_gives_up() {
        let foreground = AtomicUsize::new(0);
        let wanted = std::cell::Cell::new(true);
        let stop = || !wanted.get();
        let request = Request::new(&foreground, false, &stop);
        assert!(request.check().is_ok());
        wanted.set(false);
        assert!(request.check().is_err());
        assert!(request.lock(&Mutex::new(())).is_err());
    }

    #[test]
    fn only_languages_the_model_handles_are_offered() {
        assert_eq!(language("en-US").unwrap(), "en");
        assert_eq!(language("yue-CN").unwrap(), "zh");
        assert!(language("ml-IN").is_err());
        assert!(language("hi-IN").is_err());
    }

    #[test]
    fn a_stray_core_coder_flag_is_cleared_for_aac_lc() {
        let mut params = symphonia::core::codecs::CodecParameters::new();
        params.with_extra_data(vec![0x12, 0x12].into_boxed_slice());
        assert_eq!(
            decoder_params(&params).extra_data.as_deref(),
            Some(&[0x12, 0x10][..])
        );
        // HE-AAC and longer configs are left alone.
        params.with_extra_data(vec![0x13, 0x12, 0x56, 0xe5].into_boxed_slice());
        assert_eq!(
            decoder_params(&params).extra_data.as_deref(),
            Some(&[0x13, 0x12, 0x56, 0xe5][..])
        );
    }

    #[test]
    fn prefetch_runs_only_when_idle() {
        let foreground = AtomicUsize::new(0);
        let context = Mutex::new(());
        let prefetch = Request::new(&foreground, true, &|| false);
        assert!(prefetch.check().is_ok());
        drop(prefetch.lock(&context).unwrap());

        let guard = context.lock().unwrap();
        assert!(prefetch.lock(&context).is_err());
        drop(guard);

        let first = Request::new(&foreground, false, &|| false);
        let second = Request::new(&foreground, false, &|| false);
        assert!(prefetch.lock(&context).is_err());
        assert!(first.lock(&context).is_ok());
        drop(first);
        assert!(prefetch.check().is_err());
        drop(second);
        assert!(prefetch.lock(&context).is_ok());
    }

    #[test]
    fn prefetch_yields_at_next_window_to_waiting_foreground() {
        let foreground = AtomicUsize::new(0);
        let context = Mutex::new(());
        let prefetch = Request::new(&foreground, true, &|| false);
        std::thread::scope(|scope| {
            let guard = prefetch.lock(&context).unwrap();
            let (started, waiting) = std::sync::mpsc::channel();
            let foreground = &foreground;
            let context = &context;
            let worker = scope.spawn(move || {
                let request = Request::new(foreground, false, &|| false);
                started.send(()).unwrap();
                let _guard = request.lock(context).unwrap();
                assert!(request.check().is_ok());
            });
            waiting.recv().unwrap();
            let next_window = (|| -> Result<()> {
                let _guard = guard;
                prefetch.check()?;
                panic!("Prefetch must not start another window");
            })();
            assert!(next_window.is_err());
            worker.join().unwrap();
        });
        assert_eq!(foreground.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn selects_audio_after_chapter_tracks() {
        use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters};
        let mut audio = CodecParameters::new();
        audio.codec = CODEC_TYPE_AAC;
        audio.sample_rate = Some(44_100);
        let tracks = [
            Track::new(1, CodecParameters::new()),
            Track::new(2, audio.clone()),
        ];
        assert_eq!(audio_track(&tracks).unwrap().id, 2);
        assert!(audio_track(&[]).is_err());
        assert!(audio_track(&tracks[..1]).is_err());
        audio.sample_rate = Some(0);
        assert!(audio_track(&[Track::new(3, audio)]).is_err());
    }
    #[test]
    fn locales() {
        assert_eq!(language("en-US").unwrap(), "en");
        assert_eq!(language("zh_TW").unwrap(), "zh");
        assert_eq!(language("yue-CN").unwrap(), "zh");
        assert_eq!(language("YUE-HK").unwrap(), "zh");
        assert!(language("").is_err());
        assert!(language("not-a-language").is_err());
    }
    #[test]
    fn resampling() {
        assert!(resample(&[], 44_100).is_empty());
        let output = resample(&vec![0.5; 48_000], 48_000);
        assert_eq!(output.len(), 16_000);
        assert!(output.iter().all(|x| (*x - 0.5).abs() < 0.0001));
        assert_eq!(resample(&[0.1, 0.2], 16_000), vec![0.1, 0.2]);
    }
    #[test]
    fn segment_offsets() {
        let line = absolute_line(123.5, 100, 400, 126., "words".into());
        assert_eq!(line.start, 124.5);
        assert_eq!(line.end, 126.);
    }
    #[test]
    fn invalid_ranges_and_missing_audio() {
        let path = Path::new("missing-readalong-test-audio.m4a");
        for (from, to) in [(0., 0.), (-1., 2.), (f64::NAN, 2.), (0., f64::INFINITY)] {
            assert!(transcribe(path, from, to, "en-US", &|| false, |_| {}).is_err());
        }
        assert!(transcribe(path, 0., 10., "en-US", &|| false, |_| {}).is_err());
    }
}
