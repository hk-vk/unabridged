//! Typed wrappers for Audible API endpoints.
//!
//! Each submodule exposes free async functions taking an [`HttpClient`], so
//! they compose without a god object; [`crate::client::Audible`] wires them
//! into the [`crate::client::AudibleClient`] facade.
//!
//! [`HttpClient`]: crate::http::HttpClient

pub mod catalog;
pub mod content;
pub mod library;
pub mod position;

/// Join response groups the way the official apps send them.
pub(crate) fn join_groups<S: AsRef<str>>(groups: &[S]) -> String {
    groups
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(",")
}

/// Join image sizes (`500,1215`).
pub(crate) fn join_sizes(sizes: &[u32]) -> String {
    sizes
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
