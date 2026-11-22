//! Resumable file downloads with progress callbacks.
//!
//! Data is streamed to `<dest>.part`; on success the part file is renamed to
//! `<dest>`. If the transfer is interrupted (network error, app quit, the
//! future being dropped), the next call for the same destination resumes with
//! an HTTP `Range` request. Servers that ignore `Range` (reply `200`) cause a
//! clean restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, HeaderMap, RANGE, USER_AGENT};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::http::{RetryPolicy, map_status};

/// User agent the iOS app's download manager sends (from audible-cli).
pub const DOWNLOAD_USER_AGENT: &str = "Audible/671 CFNetwork/1240.0.4 Darwin/20.6.0";

/// How often (in bytes) progress is reported while streaming.
const PROGRESS_STEP: u64 = 512 * 1024;

/// Download progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bytes on disk so far (including resumed bytes).
    pub downloaded: u64,
    /// Total size, when the server reports it.
    pub total: Option<u64>,
}

impl Progress {
    /// Completion in `0.0..=1.0`, when the total is known.
    pub fn fraction(&self) -> Option<f64> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| (self.downloaded as f64 / t as f64).min(1.0))
    }
}

/// Shared progress callback. Called from the download task; keep it cheap
/// (e.g. send to a channel the UI polls).
pub type ProgressCallback = Arc<dyn Fn(Progress) + Send + Sync>;

/// Streams files to disk with resume and retry.
#[derive(Debug, Clone)]
pub struct Downloader {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl Downloader {
    /// A downloader using `client` (share the app-wide client).
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            retry: RetryPolicy {
                max_attempts: 5,
                ..RetryPolicy::default()
            },
        }
    }

    /// Replace the retry policy (attempts cover both failed requests and
    /// interrupted streams).
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The partial-file path used for `dest`.
    pub fn part_path(dest: &Path) -> PathBuf {
        let mut name = dest
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".part");
        dest.with_file_name(name)
    }

    /// Download `url` to `dest`, resuming a previous partial download.
    /// `headers` are added to every request (e.g. signing headers).
    /// Returns the final file size.
    pub async fn download(
        &self,
        url: &str,
        headers: &HeaderMap,
        dest: &Path,
        progress: Option<ProgressCallback>,
    ) -> Result<u64> {
        if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).await?;
        }
        let part = Self::part_path(dest);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.attempt(url, headers, &part, progress.as_ref()).await {
                Ok(size) => {
                    fs::rename(&part, dest).await?;
                    info!(size, path = %dest.display(), "download complete");
                    return Ok(size);
                }
                Err(e) if e.is_transient() && attempt < self.retry.max_attempts => {
                    let delay = match &e {
                        Error::RateLimited {
                            retry_after: Some(d),
                        } => *d,
                        _ => backoff(&self.retry, attempt),
                    };
                    warn!(attempt, ?delay, error = %e, "download interrupted; resuming");
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn attempt(
        &self,
        url: &str,
        headers: &HeaderMap,
        part: &Path,
        progress: Option<&ProgressCallback>,
    ) -> Result<u64> {
        let (resp, mut file, mut downloaded, total) = loop {
            let existing = match fs::metadata(part).await {
                Ok(m) => m.len(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
                Err(e) => return Err(e.into()),
            };
            let mut req = self
                .client
                .get(url)
                .headers(headers.clone())
                .header(USER_AGENT, DOWNLOAD_USER_AGENT);
            if existing > 0 {
                req = req.header(RANGE, format!("bytes={existing}-"));
            }
            let resp = req.send().await?;
            match resp.status() {
                StatusCode::PARTIAL_CONTENT => {
                    let range = parse_content_range(resp.headers());
                    if range.map(|r| r.0) != Some(existing) {
                        warn!(
                            existing,
                            "server resumed at an unexpected offset; restarting"
                        );
                        fs::remove_file(part).await?;
                        continue;
                    }
                    let file = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(part)
                        .await?;
                    debug!(existing, "resuming download");
                    break (resp, file, existing, range.and_then(|r| r.1));
                }
                StatusCode::RANGE_NOT_SATISFIABLE if existing > 0 => {
                    let total = parse_content_range(resp.headers()).and_then(|r| r.1);
                    if total == Some(existing) {
                        return Ok(existing);
                    }
                    warn!(
                        existing,
                        ?total,
                        "partial file does not match remote file; restarting"
                    );
                    fs::remove_file(part).await?;
                }
                s if s.is_success() => {
                    let total = resp.content_length();
                    let file = File::create(part).await?;
                    break (resp, file, 0, total);
                }
                s => {
                    let retry_after = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .map(Duration::from_secs);
                    return Err(map_status(s, retry_after, format!("download failed: {s}")));
                }
            }
        };

        let report = |downloaded| {
            if let Some(cb) = progress {
                cb(Progress { downloaded, total });
            }
        };
        report(downloaded);
        let mut writer = BufWriter::with_capacity(256 * 1024, &mut file);
        let mut stream = resp.bytes_stream();
        let mut next_report = downloaded + PROGRESS_STEP;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    // Keep what we have so the retry can resume.
                    writer.flush().await?;
                    return Err(e.into());
                }
            };
            writer.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if downloaded >= next_report {
                report(downloaded);
                next_report = downloaded + PROGRESS_STEP;
            }
        }
        writer.flush().await?;
        drop(writer);
        file.sync_all().await?;
        report(downloaded);

        if let Some(total) = total
            && downloaded != total
        {
            return Err(Error::Incomplete {
                downloaded,
                expected: total,
            });
        }
        Ok(downloaded)
    }
}

fn backoff(policy: &RetryPolicy, attempt: u32) -> Duration {
    let factor = 2u32.saturating_pow(attempt.saturating_sub(1));
    policy
        .base_delay
        .saturating_mul(factor)
        .min(policy.max_delay)
}

/// Parse `Content-Range: bytes start-end/total` (or `bytes */total`) into
/// `(start, total)`.
pub(crate) fn parse_content_range(headers: &HeaderMap) -> Option<(u64, Option<u64>)> {
    let value = headers.get(CONTENT_RANGE)?.to_str().ok()?;
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let total = total.trim().parse().ok();
    let start = if range.trim() == "*" {
        total.unwrap_or(0)
    } else {
        range.split_once('-')?.0.trim().parse().ok()?
    };
    Some((start, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn range(v: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_RANGE, HeaderValue::from_static(v));
        h
    }

    #[test]
    fn parses_content_range() {
        assert_eq!(
            parse_content_range(&range("bytes 100-199/200")),
            Some((100, Some(200)))
        );
        assert_eq!(parse_content_range(&range("bytes 0-9/*")), Some((0, None)));
        assert_eq!(
            parse_content_range(&range("bytes */200")),
            Some((200, Some(200)))
        );
        assert_eq!(parse_content_range(&range("garbage")), None);
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(
            Downloader::part_path(Path::new("/tmp/a/book.aaxc")),
            PathBuf::from("/tmp/a/book.aaxc.part")
        );
    }

    #[test]
    fn fraction() {
        let p = Progress {
            downloaded: 50,
            total: Some(200),
        };
        assert_eq!(p.fraction(), Some(0.25));
        assert_eq!(
            Progress {
                downloaded: 1,
                total: None
            }
            .fraction(),
            None
        );
    }
}
