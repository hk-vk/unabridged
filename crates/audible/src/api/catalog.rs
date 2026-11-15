//! Store catalog for the Explore screen: search, browse by genre, product
//! details and the category tree.

use serde::Deserialize;

use super::library::DEFAULT_IMAGE_SIZES;
use super::{join_groups, join_sizes};
use crate::error::Result;
use crate::http::HttpClient;
use crate::models::{Asin, CatalogPage, Category, CategoryId, Product};

/// Largest page size `catalog/products` accepts.
pub const MAX_PAGE_SIZE: u32 = 50;

/// Response groups for catalog tiles and the detail sheet.
pub const DEFAULT_RESPONSE_GROUPS: &[&str] = &[
    "contributors",
    "media",
    "product_attrs",
    "product_desc",
    "product_extended_attrs",
    "rating",
    "series",
    "category_ladders",
    "customer_rights",
    "sample",
];

/// Catalog sort order (`products_sort_by`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CatalogSort {
    /// Best match for the keywords.
    #[default]
    Relevance,
    /// Best sellers first.
    BestSellers,
    /// Newest first.
    NewestFirst,
    /// Highest rated first.
    AvgRating,
    /// Title A-Z.
    Title,
}

impl CatalogSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "Relevance",
            Self::BestSellers => "BestSellers",
            Self::NewestFirst => "-ReleaseDate",
            Self::AvgRating => "AvgRating",
            Self::Title => "Title",
        }
    }
}

/// Search/browse parameters. Leave `keywords` empty and set `category_id`
/// to browse a genre.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogQuery {
    /// Free-text search.
    pub keywords: Option<String>,
    /// Restrict to an author.
    pub author: Option<String>,
    /// Restrict to a narrator.
    pub narrator: Option<String>,
    /// Restrict to a genre.
    pub category_id: Option<CategoryId>,
    /// Sort order.
    pub sort: CatalogSort,
    /// 1-based page.
    pub page: u32,
    /// Results per page (1..=50).
    pub page_size: u32,
    /// Response groups.
    pub response_groups: Vec<String>,
}

impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            keywords: None,
            author: None,
            narrator: None,
            category_id: None,
            sort: CatalogSort::default(),
            page: 1,
            page_size: 20,
            response_groups: DEFAULT_RESPONSE_GROUPS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }
}

impl CatalogQuery {
    /// Keyword search.
    pub fn search(keywords: impl Into<String>) -> Self {
        Self {
            keywords: Some(keywords.into()),
            ..Self::default()
        }
    }

    /// Browse a genre, best sellers first.
    pub fn category(id: CategoryId) -> Self {
        Self {
            category_id: Some(id),
            sort: CatalogSort::BestSellers,
            ..Self::default()
        }
    }

    fn params(&self) -> Vec<(&'static str, String)> {
        let mut q = vec![
            (
                "num_results",
                self.page_size.clamp(1, MAX_PAGE_SIZE).to_string(),
            ),
            ("page", self.page.max(1).to_string()),
            ("products_sort_by", self.sort.as_str().to_owned()),
            ("response_groups", join_groups(&self.response_groups)),
            ("image_sizes", join_sizes(DEFAULT_IMAGE_SIZES)),
        ];
        let optional = [
            ("keywords", self.keywords.clone()),
            ("author", self.author.clone()),
            ("narrator", self.narrator.clone()),
            (
                "category_id",
                self.category_id.as_ref().map(|c| c.0.clone()),
            ),
        ];
        q.extend(
            optional
                .into_iter()
                .filter_map(|(k, v)| v.filter(|s| !s.trim().is_empty()).map(|v| (k, v))),
        );
        q
    }
}

/// Search or browse the catalog.
pub async fn search(http: &HttpClient, query: &CatalogQuery) -> Result<CatalogPage> {
    http.get_json("/1.0/catalog/products", &query.params())
        .await
}

/// Full catalog details for one product.
pub async fn product(http: &HttpClient, asin: &Asin) -> Result<Product> {
    #[derive(Deserialize)]
    struct Body {
        product: Product,
    }
    let body: Body = http
        .get_json(
            &format!("/1.0/catalog/products/{asin}"),
            &[
                ("response_groups", join_groups(DEFAULT_RESPONSE_GROUPS)),
                ("image_sizes", join_sizes(DEFAULT_IMAGE_SIZES)),
            ],
        )
        .await?;
    Ok(body.product)
}

/// The genre tree under `root` (usually `"Genres"`), `levels` deep.
pub async fn categories(http: &HttpClient, root: &str, levels: u8) -> Result<Vec<Category>> {
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        categories: Vec<Category>,
    }
    let body: Body = http
        .get_json(
            "/1.0/catalog/categories",
            &[
                ("root", root.to_owned()),
                ("categories_num_levels", levels.max(1).to_string()),
                ("response_groups", "category_metadata".to_owned()),
            ],
        )
        .await?;
    Ok(body.categories)
}
