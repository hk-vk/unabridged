//! Typed errors for every fallible operation in the crate.

use std::time::Duration;

/// Convenient result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every error the library can return.
///
/// Variants are coarse enough for the UI to branch on (sign-in needed,
/// throttled, unsupported DRM, network down) while carrying a human-readable
/// message for logs. Messages never contain tokens or keys.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The transport failed (DNS, TLS, connection reset, timeout).
    #[error("network error: {0}")]
    Network(#[source] reqwest::Error),

    /// The server rejected the credentials (HTTP 401/403). Re-sign-in is
    /// usually required.
    #[error("unauthorized ({status}): {message}")]
    Unauthorized {
        /// HTTP status code.
        status: u16,
        /// Server-provided message, if any.
        message: String,
    },

    /// The pre-signed CDN URL of a license was rejected (HTTP 401, 403 or
    /// 410), almost always because it expired. Request a new license and
    /// hand its URL to [`crate::StreamSource::set_url`]; signing in again is
    /// not needed.
    #[error("the download URL expired or was rejected (HTTP {status})")]
    UrlExpired {
        /// HTTP status code.
        status: u16,
    },

    /// The requested resource does not exist (HTTP 404).
    #[error("not found: {message}")]
    NotFound {
        /// Server-provided message, if any.
        message: String,
    },

    /// HTTP 429 after all retries were exhausted.
    #[error("rate limited by Audible (retry after {retry_after:?})")]
    RateLimited {
        /// Value of the `Retry-After` header, when present.
        retry_after: Option<Duration>,
    },

    /// Audible refused a content license because the account or device is
    /// being throttled (`CustomerThrottled`). Waiting, or re-registering the
    /// device, is the documented remedy.
    #[error("Audible is throttling this account: {message}")]
    Throttled {
        /// Server-provided denial message.
        message: String,
    },

    /// Audible denied the content license for another reason.
    #[error("license denied: {message}")]
    LicenseDenied {
        /// Server-provided denial message.
        message: String,
    },

    /// The title is only available with a DRM scheme this library does not
    /// implement (for example Widevine/DASH).
    #[error("unsupported DRM scheme: {drm_type}")]
    UnsupportedDrm {
        /// The DRM type reported by Audible.
        drm_type: String,
    },

    /// A download ended before the expected number of bytes arrived. The
    /// partial file is kept, so retrying resumes.
    #[error("download incomplete: {downloaded} of {expected} bytes")]
    Incomplete {
        /// Bytes on disk.
        downloaded: u64,
        /// Bytes expected.
        expected: u64,
    },

    /// Any other non-success HTTP status.
    #[error("Audible API error ({status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Server-provided message, if any.
        message: String,
    },

    /// A response body did not match the expected shape.
    #[error("unexpected response from {context}: {source}")]
    Decode {
        /// What was being decoded.
        context: String,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Sign-in or device registration failed.
    #[error("authentication failed: {0}")]
    Auth(String),

    /// The operation needs credentials that are not available
    /// (for example no stored session, or no activation bytes).
    #[error("missing credential: {0}")]
    MissingCredential(&'static str),

    /// A cryptographic operation failed (bad key, bad padding, bad voucher).
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Reading from or writing to the credential store failed.
    #[error("credential store error: {0}")]
    CredentialStore(String),

    /// The audio container could not be parsed or rewritten.
    #[error("invalid media file: {0}")]
    InvalidMedia(String),

    /// The external `ffmpeg` fallback failed.
    #[error("ffmpeg failed: {0}")]
    Ffmpeg(String),

    /// A caller-supplied argument was invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Filesystem error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Build a [`Error::Decode`] with context.
    pub(crate) fn decode(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Decode {
            context: context.into(),
            source,
        }
    }

    /// Whether retrying the same request later may succeed.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_)
            | Self::RateLimited { .. }
            | Self::Throttled { .. }
            | Self::Incomplete { .. } => true,
            Self::Api { status, .. } => *status >= 500,
            _ => false,
        }
    }

    /// Whether the user must sign in again to recover.
    pub fn requires_sign_in(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized { .. } | Self::MissingCredential(_) | Self::Auth(_)
        )
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        // Strip the URL: signed CDN URLs embed short-lived access tokens.
        Self::Network(err.without_url())
    }
}
