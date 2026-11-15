//! Credential store trait and implementations.

use std::collections::HashMap;
use std::sync::Mutex;

use super::Credentials;
use crate::error::{Error, Result};

/// Persistent storage for [`Credentials`], keyed by a profile name (for
/// example the account e-mail or `"default"`).
///
/// Methods are synchronous: Keychain calls are fast local IPC. Call them from
/// a blocking-friendly context if a Keychain access prompt is possible.
pub trait CredentialStore: Send + Sync {
    /// Load the credentials for `profile`, or `None` if nothing is stored.
    fn load(&self, profile: &str) -> Result<Option<Credentials>>;
    /// Store (create or replace) the credentials for `profile`.
    fn save(&self, profile: &str, credentials: &Credentials) -> Result<()>;
    /// Delete the credentials for `profile`. Deleting nothing is not an error.
    fn delete(&self, profile: &str) -> Result<()>;
}

fn encode(credentials: &Credentials) -> Result<Vec<u8>> {
    serde_json::to_vec(credentials).map_err(|e| Error::CredentialStore(format!("encode: {e}")))
}

fn decode(bytes: &[u8]) -> Result<Credentials> {
    // Do not include the payload in the error: it is full of secrets.
    serde_json::from_slice(bytes).map_err(|e| {
        Error::CredentialStore(format!(
            "stored credentials are unreadable (line {}, column {})",
            e.line(),
            e.column()
        ))
    })
}

/// In-memory store for tests and previews. Data still round-trips through
/// JSON so serialisation bugs surface in tests.
#[derive(Debug, Default)]
pub struct MemoryStore {
    items: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn items(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>>> {
        self.items
            .lock()
            .map_err(|_| Error::CredentialStore("memory store lock poisoned".to_owned()))
    }
}

impl CredentialStore for MemoryStore {
    fn load(&self, profile: &str) -> Result<Option<Credentials>> {
        self.items()?.get(profile).map(|b| decode(b)).transpose()
    }

    fn save(&self, profile: &str, credentials: &Credentials) -> Result<()> {
        let bytes = encode(credentials)?;
        self.items()?.insert(profile.to_owned(), bytes);
        Ok(())
    }

    fn delete(&self, profile: &str) -> Result<()> {
        self.items()?.remove(profile);
        Ok(())
    }
}

/// macOS Keychain store: one generic-password item per profile under a fixed
/// service name, readable only by this app (default Keychain ACL).
#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub struct KeychainStore {
    service: String,
}

#[cfg(target_os = "macos")]
impl KeychainStore {
    /// Default Keychain service name.
    pub const DEFAULT_SERVICE: &'static str = "app.betteraudible.credentials";

    /// A store using [`Self::DEFAULT_SERVICE`].
    pub fn new() -> Self {
        Self::with_service(Self::DEFAULT_SERVICE)
    }

    /// A store using a custom service name (separate dev/prod items).
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[cfg(target_os = "macos")]
impl Default for KeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
impl CredentialStore for KeychainStore {
    fn load(&self, profile: &str) -> Result<Option<Credentials>> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(&self.service, profile) {
            Ok(bytes) => decode(&zeroize::Zeroizing::new(bytes)).map(Some),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(Error::CredentialStore(format!("Keychain read failed: {e}"))),
        }
    }

    fn save(&self, profile: &str, credentials: &Credentials) -> Result<()> {
        use security_framework::passwords::set_generic_password;
        let bytes = zeroize::Zeroizing::new(encode(credentials)?);
        set_generic_password(&self.service, profile, &bytes)
            .map_err(|e| Error::CredentialStore(format!("Keychain write failed: {e}")))
    }

    fn delete(&self, profile: &str) -> Result<()> {
        use security_framework::passwords::delete_generic_password;
        match delete_generic_password(&self.service, profile) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(Error::CredentialStore(format!(
                "Keychain delete failed: {e}"
            ))),
        }
    }
}
