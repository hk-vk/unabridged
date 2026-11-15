//! Library (owned titles) models for `GET /1.0/library`.

use serde::{Deserialize, Serialize};

use super::product::{ListeningStatus, Product};

/// Library bookkeeping for an owned title (`library_status`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryStatus {
    /// When the title was added to the library (RFC 3339).
    #[serde(default)]
    pub date_added: Option<String>,
    /// Pre-ordered and not yet released.
    #[serde(default)]
    pub is_preordered: Option<bool>,
    /// Purchase still being processed.
    #[serde(default)]
    pub is_pending: Option<bool>,
}

/// A title in the signed-in customer's library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryItem {
    /// Title metadata.
    #[serde(flatten)]
    pub product: Product,
    /// Purchase timestamp (RFC 3339).
    #[serde(default)]
    pub purchase_date: Option<String>,
    /// Library bookkeeping.
    #[serde(default)]
    pub library_status: Option<LibraryStatus>,
    /// Percent listened (`percent_complete` group).
    #[serde(default)]
    pub percent_complete: Option<f64>,
    /// Marked finished (`is_finished` group).
    #[serde(default)]
    pub is_finished: Option<bool>,
    /// Detailed progress (`listening_status` group).
    #[serde(default)]
    pub listening_status: Option<ListeningStatus>,
    /// `Active` or `Revoked` (returned).
    #[serde(default)]
    pub status: Option<String>,
    /// How the title was acquired (`Purchase`, `Free`, `AYCL`, ...).
    #[serde(default)]
    pub origin_type: Option<String>,
    /// Parent ASIN for podcast episodes and parts of multi-part books.
    #[serde(default)]
    pub parent_asin: Option<String>,
}

impl std::ops::Deref for LibraryItem {
    type Target = Product;
    fn deref(&self) -> &Product {
        &self.product
    }
}

/// One page of library results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryPage {
    /// Items on this page.
    pub items: Vec<LibraryItem>,
    /// Total items across all pages, from the `Total-Count` header.
    pub total_count: Option<u64>,
    /// 1-based page number that was requested.
    pub page: u32,
    /// Page size that was requested.
    pub page_size: u32,
}

impl LibraryPage {
    /// Whether another page is likely to follow.
    pub fn has_more(&self) -> bool {
        match self.total_count {
            Some(total) => u64::from(self.page) * u64::from(self.page_size) < total,
            None => self.items.len() as u64 >= u64::from(self.page_size),
        }
    }
}
