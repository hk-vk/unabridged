//! Downloadable, local Whisper transcription for machines without Apple SpeechAnalyzer.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::transcript::Line;

/// Multilingual whisper.cpp base model, quantised to eight bits.
pub const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q8_0.bin";
/// Verified Hugging Face LFS object size in bytes.
pub const MODEL_SIZE: u64 = 81_768_585;
/// SHA-256 of the model payload, not its LFS pointer.
pub const MODEL_SHA256: &str = "c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9";
/// Rounded-up decimal megabytes for download labels.
pub const MODEL_MB: u64 = MODEL_SIZE.div_ceil(1_000_000);
static CONTEXT: Mutex<Option<WhisperContext>> = Mutex::new(None);
static MODEL_IO: Mutex<()> = Mutex::new(());

/// Whether the optional local model is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No complete model is installed.
    NotDownloaded,
    /// A complete model is available locally.
    Ready,
}

fn model_path() -> Result<PathBuf> {
    Ok(crate::paths::support()?.join("Models/ggml-base-q8_0.bin"))
}

/// Inspect the downloaded model without loading it into memory.
pub fn status() -> Result<Status> {
    Ok(
        if model_path()?
            .metadata()
            .is_ok_and(|m| m.len() == MODEL_SIZE)
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
    let path = model_path()?;
    std::fs::create_dir_all(path.parent().context("No models directory")?)?;
    let part = path.with_extension("bin.part");
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut response = reqwest::Client::new()
                .get(MODEL_URL)
                .send()
                .await?
                .error_for_status()?;
            let mut file = std::fs::File::create(&part)?;
            let mut hash = Sha256::new();
            let mut received = 0u64;
            while let Some(chunk) = response.chunk().await? {
                received += chunk.len() as u64;
                ensure!(received <= MODEL_SIZE, "Speech model exceeds expected size");
                hash.update(&chunk);
                file.write_all(&chunk)?;
                progress(received as f32 / MODEL_SIZE as f32);
            }
            ensure!(received == MODEL_SIZE, "Incomplete speech model download");
            ensure!(
                hex::encode(hash.finalize()) == MODEL_SHA256,
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
    match std::fs::remove_file(model_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Convert a BCP-47 locale to a language supported by Whisper.
pub fn language(locale: &str) -> Result<String> {
    let code = locale
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    ensure!(
        whisper_rs::get_lang_id(&code).is_some(),
        "Whisper does not support language {locale}"
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

/// Decode and transcribe a range, streaming absolute-time segments after each window.
pub fn transcribe(
    path: &Path,
    from: f64,
    to: f64,
    locale: &str,
    mut on_line: impl FnMut(Line),
) -> Result<Vec<Line>> {
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
    let track = format.default_track().context("No audio track")?;
    let track_id = track.id;
    let rate = track
        .codec_params
        .sample_rate
        .context("No audio sample rate")?;
    ensure!(rate > 0, "Invalid audio sample rate");
    let time_base = track.codec_params.time_base.context("No audio time base")?;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    format.seek(
        SeekMode::Accurate,
        SeekTo::Time {
            time: Time::from(from),
            track_id: Some(track_id),
        },
    )?;
    decoder.reset();
    let mut context = CONTEXT
        .lock()
        .map_err(|_| anyhow::anyhow!("Speech engine lock failed"))?;
    if context.is_none() {
        let path = model_path()?;
        *context = Some(WhisperContext::new_with_params(
            path.to_str().context("Invalid model path")?,
            WhisperContextParameters::default(),
        )?);
    }
    let mut state = context
        .as_ref()
        .context("Speech engine unavailable")?
        .create_state()?;
    let threads =
        std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1)) as i32;
    let mut lines = Vec::new();
    let mut samples = Vec::new();
    let mut offset = from;
    let window = rate as usize * 30;
    let mut infer = |audio: &[f32], offset: f64| -> Result<()> {
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
        let decoded = decoder.decode(&packet)?;
        let channels = decoded.spec().channels.count();
        ensure!(
            channels > 0 && decoded.spec().rate == rate,
            "Audio format changed during transcription"
        );
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        for (index, frame) in buffer.samples().chunks(channels).enumerate() {
            let time = packet_start + index as f64 / f64::from(rate);
            if time >= from && time < to {
                samples.push(frame.iter().sum::<f32>() / channels as f32);
            }
        }
        while samples.len() >= window {
            infer(&samples[..window], offset)?;
            samples.drain(..window);
            offset += 30.;
        }
    }
    if !samples.is_empty() {
        infer(&samples, offset)?;
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locales() {
        assert_eq!(language("en-US").unwrap(), "en");
        assert_eq!(language("zh_TW").unwrap(), "zh");
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
            assert!(transcribe(path, from, to, "en-US", |_| {}).is_err());
        }
        assert!(transcribe(path, 0., 10., "en-US", |_| {}).is_err());
    }
}
