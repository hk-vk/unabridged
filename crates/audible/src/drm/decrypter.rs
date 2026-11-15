//! The [`Decrypter`] trait and its implementations.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::debug;

use super::convert;
use super::keys::DecryptionKey;
use crate::download::ProgressCallback;
use crate::error::{Error, Result};

/// Converts a downloaded AAX/AAXC file into a playable M4B.
#[async_trait]
pub trait Decrypter: Send + Sync {
    /// Decrypt `input` into `output` (overwritten if present).
    async fn decrypt(
        &self,
        input: &Path,
        output: &Path,
        key: &DecryptionKey,
        progress: Option<ProgressCallback>,
    ) -> Result<()>;
}

/// Pure-Rust decrypter (default). Runs on the blocking thread pool.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeDecrypter;

#[async_trait]
impl Decrypter for NativeDecrypter {
    async fn decrypt(
        &self,
        input: &Path,
        output: &Path,
        key: &DecryptionKey,
        progress: Option<ProgressCallback>,
    ) -> Result<()> {
        let (input, output, key) = (input.to_owned(), output.to_owned(), key.clone());
        tokio::task::spawn_blocking(move || {
            convert::decrypt_file(&input, &output, &key, progress.as_ref()).map(|_| ())
        })
        .await
        .map_err(|e| Error::InvalidMedia(format!("decrypt task failed: {e}")))?
    }
}

/// Fallback that shells out to `ffmpeg` (`-activation_bytes` for AAX,
/// `-audible_key`/`-audible_iv` for AAXC) and remuxes with `-c copy`.
///
/// Not used by default. It needs an ffmpeg build with the mov demuxer (all
/// standard builds) and does not report progress. Key material is passed on
/// the command line, so it is visible to other local processes for the
/// lifetime of the ffmpeg process; prefer [`NativeDecrypter`].
#[derive(Debug, Clone)]
pub struct FfmpegDecrypter {
    program: PathBuf,
}

impl FfmpegDecrypter {
    /// Use `ffmpeg` from `PATH`.
    pub fn new() -> Self {
        Self::with_program("ffmpeg")
    }

    /// Use a specific ffmpeg binary.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn key_args(key: &DecryptionKey) -> Vec<String> {
        match key {
            DecryptionKey::Aax(ab) => vec!["-activation_bytes".into(), ab.to_hex()],
            DecryptionKey::Aaxc { key, iv } => vec![
                "-audible_key".into(),
                hex::encode(key),
                "-audible_iv".into(),
                hex::encode(iv),
            ],
        }
    }
}

impl Default for FfmpegDecrypter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Decrypter for FfmpegDecrypter {
    async fn decrypt(
        &self,
        input: &Path,
        output: &Path,
        key: &DecryptionKey,
        _progress: Option<ProgressCallback>,
    ) -> Result<()> {
        debug!(program = %self.program.display(), "decrypting with ffmpeg");
        let out = Command::new(&self.program)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(Self::key_args(key))
            .arg("-i")
            .arg(input)
            .args([
                "-map",
                "0:a",
                "-map_metadata",
                "0",
                "-map_chapters",
                "0",
                "-c",
                "copy",
                "-f",
                "mp4",
            ])
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                Error::Ffmpeg(format!("could not start {}: {e}", self.program.display()))
            })?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(Error::Ffmpeg(
            stderr.lines().take(5).collect::<Vec<_>>().join("; "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drm::convert::tests::{AAX_IV, AAX_KEY, make_encrypted};

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// MD5 of the audio packets, via ffmpeg's md5 muxer.
    fn packet_md5(path: &Path) -> String {
        let out = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-map", "0:a", "-c", "copy", "-f", "md5", "-"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// Cross-check against FFmpeg's own AAX implementation: FFmpeg decrypts
    /// our synthetic AAX to the same audio packets as the original file and
    /// as the native decrypter. Skipped when ffmpeg is not installed.
    #[tokio::test]
    async fn native_and_ffmpeg_agree_on_aax() {
        if !ffmpeg_available() {
            eprintln!("ffmpeg not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let payload = hex::decode(crate::drm::aax::tests::ADRM_PAYLOAD).unwrap();
        let key = crate::crypto::hex16(AAX_KEY).unwrap();
        let iv = crate::crypto::hex16(AAX_IV).unwrap();
        let input = dir.path().join("book.aax");
        std::fs::write(
            &input,
            make_encrypted(dir.path(), Some(&payload), &key, &iv),
        )
        .unwrap();
        let ab: crate::drm::ActivationBytes = "1ceb00da".parse().unwrap();
        let k = DecryptionKey::Aax(ab);

        let native = dir.path().join("native.m4b");
        let ffmpeg = dir.path().join("ffmpeg.m4b");
        NativeDecrypter
            .decrypt(&input, &native, &k, None)
            .await
            .unwrap();
        FfmpegDecrypter::new()
            .decrypt(&input, &ffmpeg, &k, None)
            .await
            .unwrap();

        let original = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sine.m4a"
        ));
        let expected = packet_md5(original);
        assert_eq!(packet_md5(&native), expected);
        assert_eq!(packet_md5(&ffmpeg), expected);
    }

    #[tokio::test]
    async fn native_and_ffmpeg_agree_on_aaxc() {
        if !ffmpeg_available() {
            eprintln!("ffmpeg not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (key, iv) = ([0x5au8; 16], [0xa5u8; 16]);
        let input = dir.path().join("book.aaxc");
        std::fs::write(&input, make_encrypted(dir.path(), None, &key, &iv)).unwrap();
        let k = DecryptionKey::Aaxc { key, iv };
        let ffmpeg = dir.path().join("ffmpeg.m4a");
        FfmpegDecrypter::new()
            .decrypt(&input, &ffmpeg, &k, None)
            .await
            .unwrap();
        let original = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sine.m4a"
        ));
        assert_eq!(packet_md5(&ffmpeg), packet_md5(original));
    }

    #[tokio::test]
    async fn missing_ffmpeg_is_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = FfmpegDecrypter::with_program(dir.path().join("no-such-ffmpeg"))
            .decrypt(
                Path::new("in"),
                Path::new("out"),
                &DecryptionKey::Aaxc {
                    key: [0; 16],
                    iv: [0; 16],
                },
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Ffmpeg(_)));
    }
}
