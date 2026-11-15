//! Serde models for Audible API payloads.

mod catalog;
mod content;
mod de;
mod ids;
mod library;
mod position;
mod product;

pub use catalog::{CatalogPage, Category};
pub use content::{
    Chapter, ChapterInfo, ContentMetadata, ContentReference, ContentUrl, LicenseDenialReason,
    Quality,
};
pub(crate) use content::{ContentMetadataResponse, LicenseResponse, RawContentLicense};
pub use ids::{Asin, CategoryId};
pub use library::{LibraryItem, LibraryPage, LibraryStatus};
pub(crate) use position::LastPositionsResponse;
pub use position::{LastPositionHeard, TitlePosition};
pub use product::{
    CategoryLadder, CategoryRef, Codec, Contributor, CustomerRights, ListeningStatus, Product,
    Rating, RatingDistribution, SeriesRef,
};
