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

/// File store: one JSON file per profile in a directory only the user can
/// open (0700), each file readable only by the user (0600).
///
/// Unlike the Keychain it never shows an access prompt, including after the
/// app is rebuilt or updated with a self-signed identity. The trade-off: any
/// process running as the user can read the files without asking.
#[derive(Debug, Clone)]
pub struct FileStore {
    directory: std::path::PathBuf,
}

impl FileStore {
    /// A store keeping its files in `directory`, created on first save.
    pub fn new(directory: impl Into<std::path::PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn path(&self, profile: &str) -> std::path::PathBuf {
        let safe = !profile.is_empty()
            && profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        let name = if safe {
            profile.to_owned()
        } else {
            hex::encode(profile)
        };
        self.directory.join(format!("{name}.json"))
    }
}

fn file_error(action: &str, error: std::io::Error) -> Error {
    Error::CredentialStore(format!("Credential file {action} failed: {error}"))
}

impl CredentialStore for FileStore {
    fn load(&self, profile: &str) -> Result<Option<Credentials>> {
        match std::fs::read(self.path(profile)) {
            Ok(bytes) => decode(&zeroize::Zeroizing::new(bytes)).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(file_error("read", e)),
        }
    }

    fn save(&self, profile: &str, credentials: &Credentials) -> Result<()> {
        use std::io::Write as _;
        #[cfg(unix)]
        use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};

        let bytes = zeroize::Zeroizing::new(encode(credentials)?);
        let mut directory = std::fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        directory.mode(0o700);
        directory
            .create(&self.directory)
            .map_err(|e| file_error("folder", e))?;
        #[cfg(unix)]
        std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| file_error("folder", e))?;
        // Write a private temporary file, then rename it over the old one, so
        // a crash never leaves half a file.
        let path = self.path(profile);
        let temporary = path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&temporary);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(&temporary)
            .map_err(|e| file_error("write", e))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| file_error("write", e))?;
        std::fs::rename(&temporary, &path).map_err(|e| file_error("write", e))
    }

    fn delete(&self, profile: &str) -> Result<()> {
        match std::fs::remove_file(self.path(profile)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(file_error("delete", e)),
        }
    }
}

#[cfg(test)]
mod file_store_tests {
    use super::*;

    #[test]
    fn corrupt_credentials_report_an_error_without_the_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path());
        std::fs::write(store.path("default"), "not-json-secret").unwrap();
        let error = store.load("default").unwrap_err().to_string();
        assert!(!error.contains("not-json-secret"));
    }

    #[test]
    fn missing_profile_loads_none_and_deletes_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path().join("credentials"));
        assert!(store.load("default").unwrap().is_none());
        store.delete("default").unwrap();
    }

    #[test]
    fn unsafe_profile_names_stay_inside_the_folder() {
        let store = FileStore::new("/x/credentials");
        assert_eq!(
            store.path("../evil"),
            std::path::Path::new("/x/credentials").join(format!("{}.json", hex::encode("../evil")))
        );
    }
}
