//! Product (title) metadata shared by the library and catalog endpoints.
//!
//! Every field is optional: which fields are present depends on the
//! `response_groups` the request asked for, and Audible omits or nulls fields
//! freely. Dates are kept as the strings Audible sends (RFC 3339 timestamps or
//! `YYYY-MM-DD` dates) so a format change cannot break deserialisation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ids::{Asin, CategoryId};

/// A person credited on a title (author or narrator).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contributor {
    /// Contributor ASIN; narrators usually have none.
    #[serde(default)]
    pub asin: Option<String>,
    /// Display name.
    pub name: String,
}

/// Series membership of a title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesRef {
    /// Series ASIN.
    #[serde(default)]
    pub asin: Option<String>,
    /// Series title.
    #[serde(default)]
    pub title: Option<String>,
    /// Position within the series as Audible formats it (`"1"`, `"2.5"`).
    #[serde(default)]
    pub sequence: Option<String>,
}

/// One rung in a category ladder (genre path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryRef {
    /// Category id.
    pub id: CategoryId,
    /// Category name.
    pub name: String,
}

/// A genre path such as *Literature & Fiction > Classics*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryLadder {
    /// Categories from the root down.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub ladder: Vec<CategoryRef>,
    /// Root name (usually `Genres`).
    #[serde(default)]
    pub root: Option<String>,
}

/// Aggregate star rating.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RatingDistribution {
    /// Mean rating (0-5).
    #[serde(default)]
    pub display_average_rating: Option<String>,
    /// Mean rating (0-5).
    #[serde(default)]
    pub average_rating: Option<f64>,
    /// Number of ratings.
    #[serde(default)]
    pub num_ratings: Option<u64>,
}

/// Ratings block from the `rating` response group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    /// Overall rating.
    #[serde(default)]
    pub overall_distribution: Option<RatingDistribution>,
    /// Number of written reviews.
    #[serde(default)]
    pub num_reviews: Option<u64>,
}

/// An AAX codec the title can be downloaded in (`available_codecs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Codec {
    /// Codec name, e.g. `aax_44_128`.
    pub name: String,
    /// Enhanced codec string, e.g. `LC_128_44100_stereo`; this is what the
    /// AAX download endpoint expects.
    #[serde(default)]
    pub enhanced_codec: Option<String>,
    /// Format label (`Enhanced`, `Format4`).
    #[serde(default)]
    pub format: Option<String>,
}

/// What the customer may do with a title (`customer_rights`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomerRights {
    /// Can be played at all.
    #[serde(default)]
    pub is_consumable: Option<bool>,
    /// Can be downloaded for offline listening.
    #[serde(default)]
    pub is_consumable_offline: Option<bool>,
    /// Access does not expire.
    #[serde(default)]
    pub is_consumable_indefinitely: Option<bool>,
}

/// Listening progress from the `listening_status` response group.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListeningStatus {
    /// Marked finished.
    #[serde(default)]
    pub is_finished: Option<bool>,
    /// Percent listened (0-100).
    #[serde(default)]
    pub percent_complete: Option<f64>,
    /// Seconds left.
    #[serde(default)]
    pub time_remaining_seconds: Option<u64>,
}

/// Title metadata common to library items and catalog products.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Product {
    /// Title ASIN.
    pub asin: Asin,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// Subtitle.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Authors (`contributors` group).
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub authors: Vec<Contributor>,
    /// Narrators (`contributors` group).
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub narrators: Vec<Contributor>,
    /// Series membership (`series` group).
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub series: Vec<SeriesRef>,
    /// Publisher.
    #[serde(default)]
    pub publisher_name: Option<String>,
    /// Release date (`YYYY-MM-DD`).
    #[serde(default)]
    pub release_date: Option<String>,
    /// Runtime in minutes.
    #[serde(default)]
    pub runtime_length_min: Option<u32>,
    /// Language (lower-case English name, e.g. `english`).
    #[serde(default)]
    pub language: Option<String>,
    /// `unabridged`, `abridged`, ...
    #[serde(default)]
    pub format_type: Option<String>,
    /// `SinglePartBook`, `MultiPartBook`, `Periodical`, `PodcastParent`, ...
    #[serde(default)]
    pub content_delivery_type: Option<String>,
    /// `Product`, `Podcast`, `Episode`, ...
    #[serde(default)]
    pub content_type: Option<String>,
    /// Short HTML blurb.
    #[serde(default)]
    pub merchandising_summary: Option<String>,
    /// Long HTML description (`product_extended_attrs`).
    #[serde(default)]
    pub publisher_summary: Option<String>,
    /// Cover URLs keyed by pixel size (`"500"`, `"1215"`), per the
    /// `image_sizes` query parameter.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub product_images: BTreeMap<String, String>,
    /// Ratings (`rating` group).
    #[serde(default)]
    pub rating: Option<Rating>,
    /// Genre paths (`category_ladders` group).
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub category_ladders: Vec<CategoryLadder>,
    /// AAX codecs (`media` group with `product_attrs`).
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub available_codecs: Vec<Codec>,
    /// Rights (`customer_rights` group).
    #[serde(default)]
    pub customer_rights: Option<CustomerRights>,
    /// Part of the Plus catalog ("all you can eat").
    #[serde(default)]
    pub is_ayce: Option<bool>,
    /// Sample audio URL (`sample` group).
    #[serde(default)]
    pub sample_url: Option<String>,
}

impl Product {
    /// The best cover URL for a requested pixel size: the smallest image at
    /// least `min_px` wide, else the largest available.
    pub fn cover_url(&self, min_px: u32) -> Option<&str> {
        let mut sized: Vec<(u32, &str)> = self
            .product_images
            .iter()
            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|px| (px, v.as_str())))
            .collect();
        sized.sort_by_key(|(px, _)| *px);
        sized
            .iter()
            .find(|(px, _)| *px >= min_px)
            .or_else(|| sized.last())
            .map(|(_, url)| *url)
    }

    /// Comma-separated author names.
    pub fn author_names(&self) -> String {
        join_names(&self.authors)
    }

    /// Comma-separated narrator names.
    pub fn narrator_names(&self) -> String {
        join_names(&self.narrators)
    }

    /// Whether Audible allows downloading this title for offline playback.
    /// Unknown (rights not requested) is treated as downloadable.
    pub fn is_downloadable(&self) -> bool {
        self.customer_rights
            .as_ref()
            .and_then(|r| r.is_consumable_offline)
            .unwrap_or(true)
    }
}

fn join_names(people: &[Contributor]) -> String {
    people
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
