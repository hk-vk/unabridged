//! Better Audible domain model.

pub mod book;
pub mod format;
pub mod progress;
pub mod provider;

/// Implementations used by the offline preview and tests.
pub mod mock {
    pub mod engine;
}
