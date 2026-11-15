//! Last-position-heard (listening progress sync) models.

use serde::{Deserialize, Serialize};

use super::ids::Asin;

/// Server-side playback position for one title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastPositionHeard {
    /// Position in milliseconds from the start of the audio.
    #[serde(default)]
    pub position_ms: Option<u64>,
    /// When the position was last written (RFC 3339).
    #[serde(default)]
    pub last_updated: Option<String>,
    /// `Exists` or `DoesNotExist`.
    #[serde(default)]
    pub status: Option<String>,
}

impl LastPositionHeard {
    /// The position, if the server has one.
    pub fn position(&self) -> Option<u64> {
        match self.status.as_deref() {
            Some("DoesNotExist") => None,
            _ => self.position_ms,
        }
    }
}

/// A title paired with its position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitlePosition {
    /// Title ASIN.
    pub asin: Asin,
    /// Its position.
    pub last_position_heard: LastPositionHeard,
}

/// Body of `GET /1.0/annotations/lastpositions`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LastPositionsResponse {
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub asin_last_position_heard_annots: Vec<TitlePosition>,
}
