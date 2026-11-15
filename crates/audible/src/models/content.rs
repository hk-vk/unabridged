//! Content metadata (chapters, content reference) and license models.

use serde::{Deserialize, Serialize};

use super::position::LastPositionHeard;

/// A chapter; nested chapters appear when `chapter_titles_type=Tree`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    /// Chapter title.
    #[serde(default)]
    pub title: String,
    /// Offset from the start of the audio in milliseconds.
    pub start_offset_ms: u64,
    /// Length in milliseconds.
    pub length_ms: u64,
    /// Sub-chapters.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub chapters: Vec<Chapter>,
}

/// Chapter table for a title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterInfo {
    /// Top-level chapters.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub chapters: Vec<Chapter>,
    /// Total runtime in milliseconds.
    #[serde(default)]
    pub runtime_length_ms: Option<u64>,
    /// Length of the "This is Audible" intro.
    #[serde(default, rename = "brandIntroDurationMs")]
    pub brand_intro_duration_ms: Option<u64>,
    /// Length of the outro.
    #[serde(default, rename = "brandOutroDurationMs")]
    pub brand_outro_duration_ms: Option<u64>,
    /// Whether the chapter offsets are exact.
    #[serde(default)]
    pub is_accurate: Option<bool>,
}

impl ChapterInfo {
    /// Chapters flattened depth-first (parents before their children).
    pub fn flatten(&self) -> Vec<&Chapter> {
        fn walk<'a>(chapters: &'a [Chapter], out: &mut Vec<&'a Chapter>) {
            for c in chapters {
                out.push(c);
                walk(&c.chapters, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.chapters, &mut out);
        out
    }
}

/// Identifies one concrete encoding of a title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentReference {
    /// Audible Content Reference; required when writing positions.
    pub acr: String,
    /// Title ASIN.
    #[serde(default)]
    pub asin: Option<String>,
    /// Codec, e.g. `mp4a.40.2`.
    #[serde(default)]
    pub codec: Option<String>,
    /// Container/format, e.g. `AAX_44_128`.
    #[serde(default)]
    pub content_format: Option<String>,
    /// File size in bytes.
    #[serde(default)]
    pub content_size_in_bytes: Option<u64>,
    /// Encoding version.
    #[serde(default)]
    pub version: Option<String>,
}

/// `content_metadata` object returned by the metadata and license endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentMetadata {
    /// Chapters (`chapter_info` group).
    #[serde(default)]
    pub chapter_info: Option<ChapterInfo>,
    /// Content reference (`content_reference` group).
    #[serde(default)]
    pub content_reference: Option<ContentReference>,
    /// Last position (`last_position_heard` group).
    #[serde(default)]
    pub last_position_heard: Option<LastPositionHeard>,
    /// Download URL (license responses only).
    #[serde(default)]
    pub content_url: Option<ContentUrl>,
}

/// Download location from a license response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentUrl {
    /// Pre-signed CDN URL for the encrypted file. Treat as a secret: it
    /// grants access to the file until it expires.
    #[serde(default)]
    pub offline_url: Option<String>,
}

/// Body of `GET /1.0/content/{asin}/metadata`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ContentMetadataResponse {
    pub content_metadata: ContentMetadata,
}

/// Requested audio quality for licenses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    /// Highest bitrate available.
    #[default]
    High,
    /// Lower bitrate.
    Normal,
}

impl Quality {
    /// Value sent to the API.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Normal => "Normal",
        }
    }
}

/// Reason attached to a denied license.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LicenseDenialReason {
    /// Human-readable message.
    #[serde(default)]
    pub message: Option<String>,
    /// Machine reason, e.g. `CustomerThrottled`.
    #[serde(default, rename = "rejectionReason")]
    pub rejection_reason: Option<String>,
    /// What was validated, e.g. `Membership`.
    #[serde(default, rename = "validationType")]
    pub validation_type: Option<String>,
}

/// Raw `content_license` object.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawContentLicense {
    #[serde(default)]
    pub asin: Option<String>,
    #[serde(default)]
    pub status_code: Option<String>,
    #[serde(default)]
    pub drm_type: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub license_denial_reasons: Vec<LicenseDenialReason>,
    #[serde(default)]
    pub content_metadata: Option<ContentMetadata>,
    /// Base64 AES-encrypted voucher (AAXC only).
    #[serde(default)]
    pub license_response: Option<String>,
}

/// Body of `POST /1.0/content/{asin}/licenserequest`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LicenseResponse {
    pub content_license: RawContentLicense,
}
