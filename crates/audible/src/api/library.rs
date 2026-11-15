//! `GET /1.0/library` and `GET /1.0/library/{asin}`.

use reqwest::Method;
use serde::Deserialize;

use super::{join_groups, join_sizes};
use crate::error::Result;
use crate::http::{Host, HttpClient};
use crate::models::{Asin, LibraryItem, LibraryPage};

/// Largest page size the endpoint accepts.
pub const MAX_PAGE_SIZE: u32 = 1000;

/// Response groups requested by default: enough for the library grid, the
/// detail screen and download decisions.
pub const DEFAULT_RESPONSE_GROUPS: &[&str] = &[
    "contributors",
    "customer_rights",
    "media",
    "product_attrs",
    "product_desc",
    "product_extended_attrs",
    "rating",
    "series",
    "category_ladders",
    "is_finished",
    "listening_status",
    "percent_complete",
];

/// Cover sizes requested by default (grid and detail/retina).
pub const DEFAULT_IMAGE_SIZES: &[u32] = &[500, 1215];

/// Library sort order (`sort_by`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LibrarySort {
    /// Most recently purchased first.
    #[default]
    PurchaseDateDesc,
    /// Oldest purchase first.
    PurchaseDateAsc,
    /// Title A-Z.
    Title,
    /// Author A-Z.
    Author,
    /// Shortest first.
    Length,
}

impl LibrarySort {
    fn as_str(self) -> &'static str {
        match self {
            Self::PurchaseDateDesc => "-PurchaseDate",
            Self::PurchaseDateAsc => "PurchaseDate",
            Self::Title => "Title",
            Self::Author => "Author",
            Self::Length => "Length",
        }
    }
}

/// Parameters for listing the library.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryQuery {
    /// Items per page (1..=1000).
    pub page_size: u32,
    /// Response groups.
    pub response_groups: Vec<String>,
    /// Cover sizes to include in `product_images`.
    pub image_sizes: Vec<u32>,
    /// Sort order.
    pub sort: LibrarySort,
    /// Only items purchased after this RFC 3339 timestamp (incremental sync).
    pub purchased_after: Option<String>,
    /// Children of a multi-part book or podcast.
    pub parent_asin: Option<Asin>,
}

impl Default for LibraryQuery {
    fn default() -> Self {
        Self {
            page_size: 200,
            response_groups: DEFAULT_RESPONSE_GROUPS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            image_sizes: DEFAULT_IMAGE_SIZES.to_vec(),
            sort: LibrarySort::default(),
            purchased_after: None,
            parent_asin: None,
        }
    }
}

impl LibraryQuery {
    fn params(&self, page: u32) -> Vec<(&'static str, String)> {
        let mut q = vec![
            (
                "num_results",
                self.page_size.clamp(1, MAX_PAGE_SIZE).to_string(),
            ),
            ("page", page.max(1).to_string()),
            ("response_groups", join_groups(&self.response_groups)),
            ("sort_by", self.sort.as_str().to_owned()),
        ];
        if !self.image_sizes.is_empty() {
            q.push(("image_sizes", join_sizes(&self.image_sizes)));
        }
        if let Some(after) = &self.purchased_after {
            q.push(("purchased_after", after.clone()));
        }
        if let Some(parent) = &self.parent_asin {
            q.push(("parent_asin", parent.to_string()));
        }
        q
    }
}

#[derive(Deserialize)]
struct ItemsBody {
    #[serde(default)]
    items: Vec<LibraryItem>,
}

#[derive(Deserialize)]
struct ItemBody {
    item: LibraryItem,
}

/// Fetch one page (1-based) of the library.
pub async fn page(http: &HttpClient, query: &LibraryQuery, page: u32) -> Result<LibraryPage> {
    let url = http.url(Host::Api, "/1.0/library", &query.params(page))?;
    let resp = http.execute(Method::GET, url, None, &[]).await?;
    let total_count = resp
        .headers
        .get("total-count")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body: ItemsBody = resp.json("library")?;
    Ok(LibraryPage {
        items: body.items,
        total_count,
        page: page.max(1),
        page_size: query.page_size.clamp(1, MAX_PAGE_SIZE),
    })
}

/// Fetch every page of the library, calling `on_page` after each one so the
/// UI can render progressively.
pub async fn all(
    http: &HttpClient,
    query: &LibraryQuery,
    mut on_page: impl FnMut(&LibraryPage),
) -> Result<Vec<LibraryItem>> {
    let mut items = Vec::new();
    let mut n = 1;
    loop {
        let p = page(http, query, n).await?;
        on_page(&p);
        let more = p.has_more() && !p.items.is_empty();
        items.extend(p.items);
        if !more {
            return Ok(items);
        }
        n += 1;
    }
}

/// Fetch one library item with the given response groups
/// ([`DEFAULT_RESPONSE_GROUPS`] when empty).
pub async fn item(http: &HttpClient, asin: &Asin, response_groups: &[&str]) -> Result<LibraryItem> {
    let groups = if response_groups.is_empty() {
        DEFAULT_RESPONSE_GROUPS
    } else {
        response_groups
    };
    let body: ItemBody = http
        .get_json(
            &format!("/1.0/library/{asin}"),
            &[
                ("response_groups", join_groups(groups)),
                ("image_sizes", join_sizes(DEFAULT_IMAGE_SIZES)),
            ],
        )
        .await?;
    Ok(body.item)
}
