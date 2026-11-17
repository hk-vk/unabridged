//! Signed-in session data and where it is stored.
//!
//! [`Credentials`] holds everything a registered device needs. It is stored
//! as one JSON blob per profile through a [`CredentialStore`]:
//! [`FileStore`] (private JSON files) in the app, [`KeychainStore`] (macOS
//! Keychain, generic password item) for moving older sign-ins over, and
//! [`MemoryStore`] in tests.

mod secret;
mod store;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub use secret::Secret;
#[cfg(target_os = "macos")]
pub use store::KeychainStore;
#[cfg(unix)]
pub use store::FileStore;
pub use store::{CredentialStore, MemoryStore};

use crate::auth::DeviceSerial;
use crate::locale::Marketplace;

/// Everything a registered device needs to talk to Audible.
///
/// `Debug` output redacts every secret field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    /// Marketplace the device was registered in.
    pub marketplace: Marketplace,
    /// Serial of the registered device (needed to decrypt AAXC vouchers).
    pub device_serial: DeviceSerial,
    /// Device type id (`A2CZJZGLK2JJVM` for iPhone).
    pub device_type: String,
    /// Name Amazon gave the device, shown in "Manage your devices".
    #[serde(default)]
    pub device_name: Option<String>,
    /// Amazon customer id (`amzn1.account.…`); needed for AAXC vouchers.
    pub customer_id: String,
    /// Customer display name.
    #[serde(default)]
    pub customer_name: Option<String>,
    /// ADP token used for request signing.
    pub adp_token: Secret,
    /// PEM RSA private key used for request signing.
    pub device_private_key: Secret,
    /// Short-lived bearer token (`Atna|…`).
    pub access_token: Secret,
    /// When [`Self::access_token`] expires.
    pub access_token_expires: DateTime<Utc>,
    /// Long-lived refresh token (`Atnr|…`).
    pub refresh_token: Secret,
    /// Amazon website cookies returned at registration.
    #[serde(default)]
    pub website_cookies: BTreeMap<String, Secret>,
    /// Store authentication cookie (raw JSON), if returned.
    #[serde(default)]
    pub store_authentication_cookie: Option<Secret>,
    /// Activation bytes (8 hex chars) for AAX files, once fetched.
    #[serde(default)]
    pub activation_bytes: Option<Secret>,
}

impl Credentials {
    /// Whether the access token expires within `margin`.
    pub fn access_token_expires_within(&self, margin: Duration) -> bool {
        self.access_token_expires - margin <= Utc::now()
    }
}
