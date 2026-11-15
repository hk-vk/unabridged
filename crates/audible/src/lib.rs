//! Backend library for Better Audible, a native macOS Audible client.
//!
//! The crate lets an app behave like the official Audible iOS app for the
//! signed-in owner's purchased content:
//!
//! * [`auth`]: OAuth 2 (PKCE) sign-in URL, device registration, token refresh.
//! * [`credentials`]: credential storage in the macOS Keychain.
//! * [`signing`]: ADP request signing with the device private key.
//! * [`http`]: API transport with signing, retries and error mapping.
//! * [`api`]: library, catalog, content (chapters, licenses), positions.
//! * [`download`]: resumable downloads with progress callbacks.
//! * [`drm`]: AAX/AAXC decryption to a playable M4B, in pure Rust with an
//!   optional ffmpeg fallback.
//! * [`client`]: the [`AudibleClient`] facade the UI talks to.
//!
//! See `README.md` for protocol notes and known uncertainties.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
#![warn(missing_docs)]

pub mod api;
pub mod auth;
pub mod credentials;
mod crypto;
pub mod drm;
pub mod error;
pub mod http;
pub mod locale;
pub mod models;
pub mod signing;

pub use error::{Error, Result};
pub use locale::{Endpoints, Marketplace};
pub use models::Asin;
