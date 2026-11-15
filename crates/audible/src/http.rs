//! API transport: request signing, retries with backoff, error mapping.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, RETRY_AFTER};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::{debug, warn};
use url::Url;

use crate::error::{Error, Result};
use crate::locale::Endpoints;
use crate::signing::AdpSigner;

/// Which host a request goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// `api.audible.<tld>`.
    Api,
    /// `www.audible.<tld>`.
    AudibleWww,
    /// `www.audible.com` (activation blob).
    Activation,
}

/// Retry behaviour for transient failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first (1 disables retries).
    pub max_attempts: u32,
    /// Delay before the first retry; doubles on each further retry.
    pub base_delay: Duration,
    /// Upper bound for any single delay, including `Retry-After`.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// No retries (useful in tests).
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    fn backoff(&self, retry: u32) -> Duration {
        let factor = 2u32.saturating_pow(retry.saturating_sub(1));
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }
}

/// A response that was fully read.
#[derive(Debug, Clone)]
pub struct RawResponse {
    /// Status code (always a success status when returned by [`HttpClient`]).
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Body bytes.
    pub body: Bytes,
}

impl RawResponse {
    /// Decode the body as JSON.
    pub fn json<T: DeserializeOwned>(&self, context: &str) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|e| Error::decode(context, e))
    }
}

/// Signed HTTP client for the Audible API. Cheap to clone.
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    endpoints: Arc<Endpoints>,
    signer: Arc<AdpSigner>,
    retry: RetryPolicy,
}

impl HttpClient {
    /// Build a client. `inner` should be shared app-wide.
    pub fn new(inner: reqwest::Client, endpoints: Endpoints, signer: AdpSigner) -> Self {
        Self {
            inner,
            endpoints: Arc::new(endpoints),
            signer: Arc::new(signer),
            retry: RetryPolicy::default(),
        }
    }

    /// Replace the retry policy.
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The endpoints this client talks to.
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// The underlying reqwest client (for unsigned CDN downloads).
    pub fn reqwest(&self) -> &reqwest::Client {
        &self.inner
    }

    /// The request signer.
    pub fn signer(&self) -> &AdpSigner {
        &self.signer
    }

    /// Absolute URL for a path on `host`, with query parameters.
    pub fn url(&self, host: Host, path: &str, query: &[(&str, String)]) -> Result<Url> {
        let base = match host {
            Host::Api => &self.endpoints.api,
            Host::AudibleWww => &self.endpoints.audible_www,
            Host::Activation => &self.endpoints.activation,
        };
        let mut url = Url::parse(&format!("{base}{path}"))
            .map_err(|e| Error::InvalidArgument(format!("bad URL for {path}: {e}")))?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        Ok(url)
    }

    /// Signing headers for `method url body`.
    pub fn sign(&self, method: &Method, url: &Url, body: &[u8]) -> HeaderMap {
        let target = match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_owned(),
        };
        let signed = self.signer.sign(method.as_str(), &target, body);
        let mut headers = HeaderMap::new();
        for (name, value) in signed.pairs() {
            if let Ok(value) = value.parse() {
                headers.insert(name, value);
            }
        }
        headers
    }

    /// `GET` JSON from the API host.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let url = self.url(Host::Api, path, query)?;
        self.execute(Method::GET, url, None, &[]).await?.json(path)
    }

    /// `POST` a JSON body to the API host and decode the JSON reply.
    pub async fn post_json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        extra_headers: &[(&'static str, String)],
    ) -> Result<T> {
        let url = self.url(Host::Api, path, &[])?;
        let body = serde_json::to_vec(body).map_err(|e| Error::decode(path, e))?;
        self.execute(Method::POST, url, Some(body), extra_headers)
            .await?
            .json(path)
    }

    /// `PUT` a JSON body to the API host; the reply body is ignored.
    pub async fn put_json<B: Serialize + ?Sized>(&self, path: &str, body: &B) -> Result<()> {
        let url = self.url(Host::Api, path, &[])?;
        let body = serde_json::to_vec(body).map_err(|e| Error::decode(path, e))?;
        self.execute(Method::PUT, url, Some(body), &[]).await?;
        Ok(())
    }

    /// Send a signed request with retries, returning the full response on
    /// success and a typed error otherwise.
    pub async fn execute(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
        extra_headers: &[(&'static str, String)],
    ) -> Result<RawResponse> {
        let idempotent = matches!(
            method,
            Method::GET | Method::PUT | Method::HEAD | Method::DELETE
        );
        let body_bytes = body.unwrap_or_default();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self
                .send_once(&method, &url, &body_bytes, extra_headers)
                .await;
            let retry_after = match &result {
                Ok(_) => None,
                Err(Failure::Status {
                    status,
                    retry_after,
                    ..
                }) if should_retry_status(*status, idempotent) => Some(*retry_after),
                Err(Failure::Transport(e)) if idempotent && (e.is_connect() || e.is_timeout()) => {
                    Some(None)
                }
                Err(_) => None,
            };
            match (result, retry_after) {
                (Ok(resp), _) => return Ok(resp),
                (Err(failure), Some(retry_after)) if attempt < self.retry.max_attempts => {
                    let delay = retry_after
                        .unwrap_or_else(|| self.retry.backoff(attempt))
                        .min(self.retry.max_delay);
                    warn!(%method, path = url.path(), attempt, ?delay, reason = %failure.summary(), "retrying request");
                    tokio::time::sleep(delay).await;
                }
                (Err(failure), _) => return Err(failure.into_error()),
            }
        }
    }

    async fn send_once(
        &self,
        method: &Method,
        url: &Url,
        body: &[u8],
        extra_headers: &[(&'static str, String)],
    ) -> std::result::Result<RawResponse, Failure> {
        let mut req = self
            .inner
            .request(method.clone(), url.clone())
            .headers(self.sign(method, url, body))
            .header(ACCEPT, "application/json")
            .header("Accept-Charset", "utf-8");
        if !body.is_empty() {
            req = req
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        for (name, value) in extra_headers {
            req = req.header(*name, value.as_str());
        }
        let resp = req.send().await.map_err(Failure::Transport)?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.bytes().await.map_err(Failure::Transport)?;
        debug!(%method, path = url.path(), status = status.as_u16(), "api response");
        if status.is_success() {
            return Ok(RawResponse {
                status,
                headers,
                body,
            });
        }
        Err(Failure::Status {
            status,
            retry_after: parse_retry_after(&headers),
            message: error_message(&body),
        })
    }
}

/// Internal failure before mapping to [`Error`].
enum Failure {
    Transport(reqwest::Error),
    Status {
        status: StatusCode,
        retry_after: Option<Duration>,
        message: String,
    },
}

impl Failure {
    fn summary(&self) -> String {
        match self {
            Self::Transport(e) => e.to_string(),
            Self::Status { status, .. } => status.to_string(),
        }
    }

    fn into_error(self) -> Error {
        match self {
            Self::Transport(e) => Error::from(e),
            Self::Status {
                status,
                retry_after,
                message,
            } => map_status(status, retry_after, message),
        }
    }
}

fn should_retry_status(status: StatusCode, idempotent: bool) -> bool {
    match status.as_u16() {
        // The server did not process the request: safe for any method.
        429 | 503 => true,
        500 | 502 | 504 => idempotent,
        _ => false,
    }
}

/// Map a non-success status to a typed error.
pub(crate) fn map_status(
    status: StatusCode,
    retry_after: Option<Duration>,
    message: String,
) -> Error {
    match status.as_u16() {
        401 | 403 => Error::Unauthorized {
            status: status.as_u16(),
            message,
        },
        404 => Error::NotFound { message },
        429 => Error::RateLimited { retry_after },
        code => Error::Api {
            status: code,
            message,
        },
    }
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Pull a short message out of an Audible error body
/// (`{"message": ...}` or `{"error": ...}`), else a truncated body.
fn error_message(body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        for key in ["message", "error_description", "error"] {
            if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
                return text.to_owned();
            }
        }
    }
    let text = String::from_utf8_lossy(body);
    let mut out: String = text.chars().take(200).collect();
    if text.chars().count() > 200 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy {
            max_attempts: 10,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
        };
        assert_eq!(p.backoff(1), Duration::from_millis(100));
        assert_eq!(p.backoff(2), Duration::from_millis(200));
        assert_eq!(p.backoff(3), Duration::from_millis(400));
        assert_eq!(p.backoff(4), Duration::from_millis(500));
        assert_eq!(p.backoff(40), Duration::from_millis(500));
    }

    #[test]
    fn extracts_messages() {
        assert_eq!(
            error_message(br#"{"message":"Invalid asin"}"#),
            "Invalid asin"
        );
        assert_eq!(error_message(br#"{"error":"bad"}"#), "bad");
        assert_eq!(error_message(b"<html>oops</html>"), "<html>oops</html>");
        assert_eq!(error_message(&[b'x'; 300]).chars().count(), 201);
    }

    #[test]
    fn maps_statuses() {
        assert!(matches!(
            map_status(StatusCode::FORBIDDEN, None, String::new()),
            Error::Unauthorized { status: 403, .. }
        ));
        assert!(matches!(
            map_status(StatusCode::NOT_FOUND, None, String::new()),
            Error::NotFound { .. }
        ));
        assert!(matches!(
            map_status(StatusCode::TOO_MANY_REQUESTS, None, String::new()),
            Error::RateLimited { .. }
        ));
        assert!(matches!(
            map_status(StatusCode::BAD_REQUEST, None, String::new()),
            Error::Api { status: 400, .. }
        ));
    }

    #[test]
    fn retry_rules() {
        assert!(should_retry_status(StatusCode::TOO_MANY_REQUESTS, false));
        assert!(should_retry_status(StatusCode::SERVICE_UNAVAILABLE, false));
        assert!(!should_retry_status(StatusCode::BAD_GATEWAY, false));
        assert!(should_retry_status(StatusCode::BAD_GATEWAY, true));
        assert!(!should_retry_status(StatusCode::BAD_REQUEST, true));
    }
}
