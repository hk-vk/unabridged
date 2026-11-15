//! Catalog (store) models for search and browse.

use serde::{Deserialize, Serialize};

use super::ids::CategoryId;
use super::product::Product;

/// One page of catalog products.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogPage {
    /// Products on this page.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub products: Vec<Product>,
    /// Total matches across all pages, when Audible reports it.
    #[serde(default)]
    pub total_results: Option<u64>,
}

/// A catalog category (genre) node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category {
    /// Category id, used for browsing products by genre.
    pub id: CategoryId,
    /// Display name.
    pub name: String,
    /// Sub-categories, when `categories_num_levels` > 1.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub children: Vec<Category>,
    /// Featured products, when the `products` response group was requested.
    #[serde(default, deserialize_with = "super::de::null_default")]
    pub products: Vec<Product>,
}
