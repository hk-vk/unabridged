//! Account and library operations, run away from GPUI's main thread.

use audible::{
    Audible, AudibleClient, auth::LoginSession, credentials::CredentialStore, models::LibraryItem,
};
use std::sync::Arc;

const PROFILE: &str = "default";

// Keep credential storage access to the beginning of a session and to real
// credential changes. Reads from older Keychain sign-ins can show a macOS
// access prompt, so:
// * a successful read is cached for the session;
// * a failed or cancelled read is remembered too, so background work (progress
//   sync, chapters) does not ask again; an explicit restore clears it;
// * a refreshed access token is kept in memory only. The refresh token stays
//   valid, so the next launch refreshes again instead of rewriting the item.
struct SessionStore {
    inner: Arc<dyn CredentialStore>,
    state: std::sync::Mutex<SessionState>,
}

#[derive(Default)]
struct SessionState {
    loaded: std::collections::HashMap<String, Option<audible::credentials::Credentials>>,
    failed: std::collections::HashMap<String, String>,
}

impl SessionStore {
    fn new(inner: Arc<dyn CredentialStore>) -> Self {
        Self {
            inner,
            state: Default::default(),
        }
    }

    fn state(&self) -> audible::Result<std::sync::MutexGuard<'_, SessionState>> {
        self.state
            .lock()
            .map_err(|_| audible::Error::CredentialStore("session store lock failed".into()))
    }

    /// Forgets a failed read so the next load asks the Keychain again.
    fn retry(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.failed.clear();
        }
    }
}

/// Whether `new` differs from `old` only in the short-lived access token.
fn only_access_token_changed(
    old: &audible::credentials::Credentials,
    new: &audible::credentials::Credentials,
) -> bool {
    let mut old = old.clone();
    old.access_token = new.access_token.clone();
    old.access_token_expires = new.access_token_expires;
    &old == new
}

impl CredentialStore for SessionStore {
    fn load(&self, profile: &str) -> audible::Result<Option<audible::credentials::Credentials>> {
        let mut state = self.state()?;
        if let Some(value) = state.loaded.get(profile) {
            return Ok(value.clone());
        }
        if let Some(message) = state.failed.get(profile) {
            return Err(audible::Error::CredentialStore(message.clone()));
        }
        match self.inner.load(profile) {
            Ok(value) => {
                state.loaded.insert(profile.to_owned(), value.clone());
                Ok(value)
            }
            Err(error) => {
                state.failed.insert(profile.to_owned(), error.to_string());
                Err(error)
            }
        }
    }

    fn save(
        &self,
        profile: &str,
        credentials: &audible::credentials::Credentials,
    ) -> audible::Result<()> {
        let mut state = self.state()?;
        let unchanged = matches!(
            state.loaded.get(profile),
            Some(Some(old)) if only_access_token_changed(old, credentials)
        );
        if !unchanged && let Err(error) = self.inner.save(profile, credentials) {
            state.loaded.remove(profile);
            return Err(error);
        }
        state.failed.remove(profile);
        state
            .loaded
            .insert(profile.to_owned(), Some(credentials.clone()));
        Ok(())
    }

    fn delete(&self, profile: &str) -> audible::Result<()> {
        let mut state = self.state()?;
        if let Err(error) = self.inner.delete(profile) {
            state.loaded.remove(profile);
            return Err(error);
        }
        state.failed.remove(profile);
        state.loaded.insert(profile.to_owned(), None);
        Ok(())
    }
}

/// The process-wide session store.
fn session() -> &'static Arc<SessionStore> {
    static STORE: std::sync::OnceLock<Arc<SessionStore>> = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let inner: Arc<dyn CredentialStore> = Arc::new(MovedFromKeychain {
            file: audible::credentials::FileStore::new(credentials_directory()),
            keychain: audible::credentials::KeychainStore::new(),
        });
        #[cfg(not(target_os = "macos"))]
        let inner: Arc<dyn CredentialStore> =
            Arc::new(audible::credentials::FileStore::new(credentials_directory()));
        Arc::new(SessionStore::new(inner))
    })
}

/// Where the sign-in is kept: private files, not the Keychain. The app is
/// signed with a self-signed identity, which macOS does not trust, so the
/// Keychain asked for access again after every rebuild and update.
fn credentials_directory() -> std::path::PathBuf {
    crate::paths::support()
        .expect("Application data directory not found")
        .join("Credentials")
}

/// The file store, which moves a sign-in saved by older versions out of the
/// Keychain the first time it is read (one last access prompt).
#[cfg(target_os = "macos")]
struct MovedFromKeychain {
    file: audible::credentials::FileStore,
    keychain: audible::credentials::KeychainStore,
}

#[cfg(target_os = "macos")]
impl CredentialStore for MovedFromKeychain {
    fn load(&self, profile: &str) -> audible::Result<Option<audible::credentials::Credentials>> {
        if let Some(credentials) = self.file.load(profile)? {
            return Ok(Some(credentials));
        }
        let Some(credentials) = self.keychain.load(profile)? else {
            return Ok(None);
        };
        self.file.save(profile, &credentials)?;
        let _ = self.keychain.delete(profile);
        Ok(Some(credentials))
    }

    fn save(
        &self,
        profile: &str,
        credentials: &audible::credentials::Credentials,
    ) -> audible::Result<()> {
        self.file.save(profile, credentials)
    }

    fn delete(&self, profile: &str) -> audible::Result<()> {
        self.file.delete(profile)?;
        self.keychain.delete(profile)
    }
}

fn store() -> anyhow::Result<Arc<dyn CredentialStore>> {
    Ok(session().clone())
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Library {
    pub items: Vec<LibraryItem>,
    pub marketplace: String,
    pub account_key: String,
    #[serde(skip)]
    pub offline: bool,
    /// When each book was last listened to (Unix seconds), on this Mac or
    /// any device that syncs with Audible. Books in progress are ordered by it.
    #[serde(default)]
    pub listened: std::collections::HashMap<String, i64>,
}

impl Library {
    /// Books in progress, most recently listened to first; books with no
    /// known time keep the library's order after them.
    pub fn in_progress(&self) -> Vec<&LibraryItem> {
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|item| {
                crate::books::book_progress(item).unwrap_or(0.) > 0.
                    && !crate::books::book_finished(item)
            })
            .collect();
        self.sort_by_listened(&mut items);
        items
    }

    /// Most recently listened to first, keeping the order of the rest.
    pub fn sort_by_listened(&self, items: &mut [&LibraryItem]) {
        items.sort_by_key(|item| {
            std::cmp::Reverse(self.listened.get(item.asin.as_str()).copied().unwrap_or(0))
        });
    }

    /// Records listening to `asin` just now.
    pub fn mark_listened(&mut self, asin: &audible::Asin) {
        self.listened
            .insert(asin.to_string(), chrono::Utc::now().timestamp());
    }
}

fn account_directory(key: &str) -> anyhow::Result<std::path::PathBuf> {
    anyhow::ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid account identifier"
    );
    Ok(crate::paths::support()?.join("Accounts").join(key))
}

fn write_json(path: &std::path::Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid cache path"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec(value)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn fetch_library(rt: &tokio::runtime::Runtime, client: &Audible) -> anyhow::Result<Library> {
    use sha2::{Digest, Sha256};
    let credentials = rt.block_on(client.credentials());
    let account_key = hex::encode(Sha256::digest(credentials.customer_id.as_bytes()));
    let cache = account_directory(&account_key)?.join("library.json");
    let marketplace = client.marketplace().display_name().to_string();
    match rt.block_on(client.library(&Default::default())) {
        Ok(items) => {
            let mut library = Library {
                items,
                marketplace,
                account_key,
                offline: false,
                listened: Default::default(),
            };
            library.listened = audible_listened(rt, client, &library.items);
            let _ = write_json(&cache, &library);
            remember_account(&library.account_key);
            apply_local_positions(&mut library);
            Ok(library)
        }
        Err(error) => {
            if let Ok(bytes) = std::fs::read(cache)
                && let Ok(mut library) = serde_json::from_slice::<Library>(&bytes)
                && library.account_key == account_key
            {
                library.offline = true;
                apply_local_positions(&mut library);
                return Ok(library);
            }
            Err(error.into())
        }
    }
}

/// When Audible last received a place for each book in progress, from
/// whichever device. Empty when Audible can't be reached: the order then
/// falls back to listening on this Mac.
fn audible_listened(
    rt: &tokio::runtime::Runtime,
    client: &Audible,
    items: &[LibraryItem],
) -> std::collections::HashMap<String, i64> {
    let asins: Vec<_> = items
        .iter()
        .filter(|item| {
            crate::books::book_progress(item).unwrap_or(0.) > 0.
                && !crate::books::book_finished(item)
        })
        .map(|item| item.asin.clone())
        .collect();
    if asins.is_empty() {
        return Default::default();
    }
    let Ok(positions) = rt.block_on(client.last_positions(&asins)) else {
        return Default::default();
    };
    positions
        .into_iter()
        .filter_map(|title| {
            let time = parse_audible_time(title.last_position_heard.last_updated.as_deref()?)?;
            Some((title.asin.to_string(), time))
        })
        .collect()
}

/// Audible's `last_updated`: RFC 3339, or `2024-01-01 12:34:56.789` in UTC.
fn parse_audible_time(text: &str) -> Option<i64> {
    if let Ok(time) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(time.timestamp());
    }
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f")
        .ok()
        .map(|time| time.and_utc().timestamp())
}

/// Shows places saved on this Mac in the library's progress. Audible's
/// figure lags behind (and misses places it hasn't received yet), so a local
/// place wins while it is waiting to sync or is further along; a place
/// further along on Audible (another device) keeps Audible's figure.
fn apply_local_positions(library: &mut Library) {
    for item in &mut library.items {
        let Some(saved) = local_position(&library.account_key, &item.asin) else {
            continue;
        };
        // The place's file is rewritten each time listening saves it, so its
        // time is when the book was last listened to on this Mac.
        if let Some(saved_at) = position_path(&library.account_key, &item.asin)
            .ok()
            .and_then(|path| std::fs::metadata(path).ok()?.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        {
            let saved_at = saved_at.as_secs() as i64;
            let listened = library.listened.entry(item.asin.to_string()).or_default();
            *listened = (*listened).max(saved_at);
        }
        let audible = crate::books::book_progress(item).unwrap_or(0.);
        let mut local = item.clone();
        crate::books::apply_position(&mut local, saved.millis, None);
        if saved.pending || crate::books::book_progress(&local).unwrap_or(0.) > audible {
            *item = local;
        }
    }
}

/// Where the signed-in account's key is kept, so launch can show its saved
/// library before the Keychain and Audible answer.
fn last_account_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::paths::support()?
        .join("Accounts")
        .join("last-account"))
}

fn remember_account(key: &str) {
    if let Ok(path) = last_account_path() {
        let _ = std::fs::write(path, key);
    }
}

/// The library saved on this Mac for the account signed in last time, read
/// from disk only: launch shows it at once while [`restore`] runs.
pub fn cached() -> Option<Library> {
    if std::env::var_os("BETTER_AUDIBLE_SMOKE_TEST").is_some() {
        return None;
    }
    let key = std::fs::read_to_string(last_account_path().ok()?).ok()?;
    let key = key.trim();
    let bytes = std::fs::read(account_directory(key).ok()?.join("library.json")).ok()?;
    let mut library = serde_json::from_slice::<Library>(&bytes).ok()?;
    if library.account_key != key {
        return None;
    }
    apply_local_positions(&mut library);
    Some(library)
}

/// Fetches the library again for a signed-in session, without asking the
/// Keychain anew after a denial (unlike [`restore`]).
pub fn refresh() -> anyhow::Result<Option<Library>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let Some(client) = Audible::from_store(store()?, PROFILE)? else {
        return Ok(None);
    };
    fetch_library(&rt, &client).map(Some)
}

/// Signs out of the current store: removes this Mac from the Amazon
/// account's devices when online, then deletes the saved sign-in. Downloads
/// stay on disk.
pub fn sign_out() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    if let Ok(path) = last_account_path() {
        let _ = std::fs::remove_file(path);
    }
    let store = store()?;
    let Some(client) = Audible::from_store(store.clone(), PROFILE)? else {
        return Ok(());
    };
    if rt.block_on(client.sign_out(true)).is_err() {
        // Offline or the token expired: still forget the sign-in here.
        store.delete(PROFILE)?;
    }
    Ok(())
}

pub fn restore() -> anyhow::Result<Option<Library>> {
    if std::env::var_os("BETTER_AUDIBLE_SMOKE_TEST").is_some() {
        return Ok(None);
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    // Launch and "Try loading again" are the moments to ask the Keychain anew.
    session().retry();
    let Some(client) = Audible::from_store(store()?, PROFILE)? else {
        return Ok(None);
    };
    fetch_library(&rt, &client).map(Some)
}

pub fn sign_in(session: LoginSession, redirect: String) -> anyhow::Result<Library> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = rt.block_on(Audible::sign_in(&session, &redirect, store()?, PROFILE))?;
    fetch_library(&rt, &client)
}

/// Where an account's downloaded books live.
pub fn books_directory(key: &str) -> Option<std::path::PathBuf> {
    account_directory(key)
        .ok()
        .map(|directory| directory.join("Books"))
}

pub fn downloaded_path(key: &str, asin: &audible::Asin) -> Option<std::path::PathBuf> {
    let directory = books_directory(key)?;
    playable_path(&directory, asin)
}

fn playable_path(directory: &std::path::Path, asin: &audible::Asin) -> Option<std::path::PathBuf> {
    ["m4b", "m4a"]
        .iter()
        .map(|extension| directory.join(format!("{asin}.{extension}")))
        .find(|path| path.is_file())
}

/// "314 MB", "1.2 GB".
pub fn size_label(bytes: u64) -> String {
    const GB: f64 = 1024. * 1024. * 1024.;
    const MB: f64 = 1024. * 1024.;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

/// A downloaded book and the space it takes.
#[derive(Clone, Debug)]
pub struct DownloadedBook {
    pub asin: audible::Asin,
    pub bytes: u64,
}

/// Every downloaded book for the account, largest first.
pub fn downloaded_books(key: &str) -> Vec<DownloadedBook> {
    let Some(directory) = books_directory(key) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut books: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?;
            if extension != "m4b" && extension != "m4a" {
                return None;
            }
            let asin = audible::Asin::new(path.file_stem()?.to_str()?).ok()?;
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            Some(DownloadedBook { asin, bytes })
        })
        .collect();
    books.sort_by_key(|book| std::cmp::Reverse(book.bytes));
    books
}

/// Deletes a book's audio and any leftovers of an interrupted download
/// (`.aaxc`, `.aax`, `.part`). Positions, bookmarks and read-along
/// transcripts stay, so a re-download picks up where it was.
pub fn remove_download(key: &str, asin: &audible::Asin) -> anyhow::Result<u64> {
    let directory = books_directory(key).ok_or_else(|| anyhow::anyhow!("No books folder"))?;
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(0);
    };
    let prefix = asin.to_string();
    let mut freed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // "<asin>.m4b", "<asin>.aaxc", "<asin>.aaxc.part", …
        if name.split('.').next() != Some(prefix.as_str()) {
            continue;
        }
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        std::fs::remove_file(entry.path())?;
        freed += bytes;
    }
    Ok(freed)
}

/// The signed-in client, for work that needs its own runtime (streaming).
pub(crate) fn client() -> anyhow::Result<Audible> {
    Audible::from_store(store()?, PROFILE)?.ok_or_else(|| anyhow::anyhow!("Sign in to play books"))
}

pub fn download(
    key: &str,
    asin: audible::Asin,
    quality: DownloadQuality,
    progress: Arc<std::sync::Mutex<audible::DownloadEvent>>,
) -> anyhow::Result<std::path::PathBuf> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to download books"))?;
    let mut request = audible::DownloadRequest::new(asin, account_directory(key)?.join("Books"));
    request.quality = quality.audible();
    request.progress = Some(Arc::new(move |event| {
        if let Ok(mut current) = progress.lock() {
            *current = event;
        }
    }));
    let asin = request.asin.clone();
    let path = rt.block_on(client.download_book(request))?.path;
    crate::streaming::forget(key, &asin);
    Ok(path)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedPosition {
    millis: u64,
    pending: bool,
}
fn position_path(key: &str, asin: &audible::Asin) -> anyhow::Result<std::path::PathBuf> {
    Ok(account_directory(key)?
        .join("Positions")
        .join(format!("{asin}.json")))
}
fn local_position(key: &str, asin: &audible::Asin) -> Option<SavedPosition> {
    serde_json::from_slice(&std::fs::read(position_path(key, asin).ok()?).ok()?).ok()
}

pub fn position(key: &str, asin: &audible::Asin) -> anyhow::Result<Option<u64>> {
    let local = local_position(key, asin);
    if let Some(position) = &local
        && position.pending
    {
        return Ok(Some(position.millis));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to sync listening progress"))?;
    match rt.block_on(client.last_positions(std::slice::from_ref(asin))) {
        Ok(positions) => Ok(positions
            .first()
            .and_then(|p| p.last_position_heard.position())),
        Err(error) => local.map(|p| Some(p.millis)).ok_or_else(|| error.into()),
    }
}

static SAVE_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SAVE_VERSIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, u64>>,
> = std::sync::OnceLock::new();
static NEXT_SAVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn register_save(asin: &audible::Asin) -> u64 {
    let version = NEXT_SAVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if let Ok(mut versions) = SAVE_VERSIONS.get_or_init(Default::default).lock() {
        versions.insert(asin.to_string(), version);
    }
    version
}

pub fn save_position(
    key: &str,
    asin: &audible::Asin,
    millis: u64,
    version: u64,
) -> anyhow::Result<()> {
    let _serial = SAVE_SERIAL
        .lock()
        .map_err(|_| anyhow::anyhow!("Position sync lock failed"))?;
    let is_latest = SAVE_VERSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("Position sync lock failed"))?
        .get(&asin.to_string())
        .copied()
        == Some(version);
    if !is_latest {
        return Ok(());
    }
    let path = position_path(key, asin)?;
    write_json(
        &path,
        &SavedPosition {
            millis,
            pending: true,
        },
    )?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to sync listening progress"))?;
    rt.block_on(client.set_last_position(asin, millis))?;
    let latest = SAVE_VERSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("Position sync lock failed"))?
        .get(&asin.to_string())
        .copied()
        == Some(version);
    if latest {
        write_json(
            &path,
            &SavedPosition {
                millis,
                pending: false,
            },
        )?;
    }
    Ok(())
}

/// Fetch a cover to the disposable caches directory. Covers are public images;
/// credentials are never sent with this request.
pub fn cover(asin: &audible::Asin, url: &str) -> anyhow::Result<std::path::PathBuf> {
    let directory = crate::paths::cache()?.join("Covers");
    let path = directory.join(format!("{asin}.image"));
    if path.is_file() {
        return Ok(path);
    }
    let url = reqwest::Url::parse(url)?;
    anyhow::ensure!(url.scheme() == "https", "Cover URL must use HTTPS");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let bytes = rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let mut response = client.get(url).send().await?.error_for_status()?;
        const LIMIT: usize = 5 * 1024 * 1024;
        anyhow::ensure!(
            response.content_length().is_none_or(|n| n <= LIMIT as u64),
            "Cover is too large"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(bytes.len() + chunk.len() <= LIMIT, "Cover is too large");
            bytes.extend_from_slice(&chunk);
        }
        Ok::<_, anyhow::Error>(bytes)
    })?;
    std::fs::create_dir_all(&directory)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, &path)?;
    Ok(path)
}

pub fn search_catalog(query: String) -> anyhow::Result<Vec<audible::models::Product>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to search Audible"))?;
    Ok(rt
        .block_on(client.search_catalog(&audible::api::catalog::CatalogQuery::search(query)))?
        .products)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(asin: &str, percent: f64) -> LibraryItem {
        serde_json::from_value(serde_json::json!({
            "asin": asin,
            "percent_complete": percent,
        }))
        .unwrap()
    }

    #[test]
    fn books_in_progress_follow_the_last_listen() {
        let mut library = Library {
            // Newest purchase first, as Audible returns them.
            items: vec![
                book("B000000001", 40.),
                book("B000000002", 10.),
                book("B000000003", 0.),
                book("B000000004", 20.),
                book("B000000005", 30.),
            ],
            marketplace: String::new(),
            account_key: String::new(),
            offline: false,
            listened: [("B000000004".into(), 100), ("B000000002".into(), 50)].into(),
        };
        let order = |library: &Library| {
            library
                .in_progress()
                .iter()
                .map(|item| item.asin.to_string())
                .collect::<Vec<_>>()
        };
        // Listened-to books first, the rest keep their order, unstarted left out.
        assert_eq!(
            order(&library),
            ["B000000004", "B000000002", "B000000001", "B000000005"]
        );
        library.mark_listened(&audible::Asin::new("B000000005").unwrap());
        assert_eq!(order(&library)[0], "B000000005");
    }

    #[test]
    fn audible_times_parse_in_both_forms() {
        assert_eq!(
            parse_audible_time("2026-11-01T20:14:51.357Z"),
            Some(1_793_564_091)
        );
        assert_eq!(
            parse_audible_time("2026-11-01 20:14:51.357"),
            Some(1_793_564_091)
        );
        assert_eq!(parse_audible_time("soon"), None);
    }

    #[test]
    fn playable_path_excludes_encrypted_and_partial_downloads() {
        let directory = std::env::temp_dir().join(format!(
            "better-audible-playable-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let asin = audible::Asin::new("B002V5D7RU").unwrap();
        assert!(playable_path(&directory, &asin).is_none());
        for extension in ["aax", "aaxc", "aaxc.part", "partial.m4b"] {
            std::fs::write(directory.join(format!("{asin}.{extension}")), []).unwrap();
        }
        assert!(playable_path(&directory, &asin).is_none());
        let audio = directory.join(format!("{asin}.m4b"));
        std::fs::write(&audio, []).unwrap();
        assert_eq!(playable_path(&directory, &asin), Some(audio.clone()));
        std::fs::remove_file(audio).unwrap();
        let audio = directory.join(format!("{asin}.m4a"));
        std::fs::write(&audio, []).unwrap();
        assert_eq!(playable_path(&directory, &asin), Some(audio));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn session_store_reuses_reads_and_tracks_writes_and_deletes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        #[derive(Default)]
        struct CountingStore {
            inner: audible::credentials::MemoryStore,
            reads: AtomicUsize,
            writes: AtomicUsize,
            deny: std::sync::atomic::AtomicBool,
        }
        impl CredentialStore for CountingStore {
            fn load(
                &self,
                profile: &str,
            ) -> audible::Result<Option<audible::credentials::Credentials>> {
                self.reads.fetch_add(1, Ordering::SeqCst);
                if self.deny.load(Ordering::SeqCst) {
                    return Err(audible::Error::CredentialStore("User canceled".into()));
                }
                self.inner.load(profile)
            }
            fn save(
                &self,
                profile: &str,
                credentials: &audible::credentials::Credentials,
            ) -> audible::Result<()> {
                self.writes.fetch_add(1, Ordering::SeqCst);
                self.inner.save(profile, credentials)
            }
            fn delete(&self, profile: &str) -> audible::Result<()> {
                self.inner.delete(profile)
            }
        }
        let persistent = Arc::new(CountingStore::default());
        let store = SessionStore::new(persistent.clone());
        assert!(store.load("default").unwrap().is_none());
        assert!(store.load("default").unwrap().is_none());
        assert_eq!(persistent.reads.load(Ordering::SeqCst), 1);
        let credentials: audible::credentials::Credentials =
            serde_json::from_value(serde_json::json!({
                "marketplace": "united_states",
                "device_serial": "Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0f",
                "device_type": "test", "customer_id": "test", "adp_token": "test",
                "device_private_key": "test", "access_token": "test",
                "access_token_expires": "2030-01-01T00:00:00Z", "refresh_token": "test"
            }))
            .unwrap();
        store.save("default", &credentials).unwrap();
        assert_eq!(persistent.writes.load(Ordering::SeqCst), 1);
        assert_eq!(store.load("default").unwrap(), Some(credentials.clone()));

        // A refreshed access token stays in memory: no Keychain write.
        let mut refreshed = credentials.clone();
        refreshed.access_token = "fresh".into();
        store.save("default", &refreshed).unwrap();
        assert_eq!(persistent.writes.load(Ordering::SeqCst), 1);
        assert_eq!(store.load("default").unwrap(), Some(refreshed.clone()));
        // A real change (new refresh token) is written.
        refreshed.refresh_token = "rotated".into();
        store.save("default", &refreshed).unwrap();
        assert_eq!(persistent.writes.load(Ordering::SeqCst), 2);

        store.delete("default").unwrap();
        assert!(store.load("default").unwrap().is_none());
        assert_eq!(persistent.reads.load(Ordering::SeqCst), 1);

        // A cancelled prompt is not repeated until an explicit retry.
        let denying = Arc::new(CountingStore::default());
        denying.deny.store(true, Ordering::SeqCst);
        let store = SessionStore::new(denying.clone());
        assert!(store.load("default").is_err());
        assert!(store.load("default").is_err());
        assert_eq!(denying.reads.load(Ordering::SeqCst), 1);
        store.retry();
        assert!(store.load("default").is_err());
        assert_eq!(denying.reads.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn position_cache_round_trips_and_account_paths_reject_traversal() {
        assert!(account_directory("../other-account").is_err());
        let dir =
            std::env::temp_dir().join(format!("better-audible-cache-test-{}", std::process::id()));
        let path = dir.join("position.json");
        write_json(
            &path,
            &SavedPosition {
                millis: 1234,
                pending: true,
            },
        )
        .unwrap();
        let saved: SavedPosition = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.millis, 1234);
        assert!(saved.pending);
        write_json(
            &path,
            &SavedPosition {
                millis: 4567,
                pending: false,
            },
        )
        .unwrap();
        let saved: SavedPosition = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.millis, 4567);
        assert!(!saved.pending);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bookmark_file_adds_deduplicates_and_removes_positions() {
        let directory = std::env::temp_dir().join(format!(
            "better-audible-bookmarks-test-{}",
            std::process::id()
        ));
        let path = directory.join("book.json");
        update_bookmark_file(&path, 10_000, true).unwrap();
        update_bookmark_file(&path, 5_000, true).unwrap();
        update_bookmark_file(&path, 10_000, true).unwrap();
        assert_eq!(read_bookmark_positions(&path).unwrap(), vec![5_000, 10_000]);
        update_bookmark_file(&path, 5_000, false).unwrap();
        assert_eq!(read_bookmark_positions(&path).unwrap(), vec![10_000]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn appearance_preferences_round_trip_and_unknown_values_are_safe() {
        for mode in ui::theme::Mode::ALL {
            for accent in ui::theme::AccentTheme::ALL {
                let settings = ui::ThemeSettings {
                    mode,
                    accent,
                    tint_with_cover: false,
                    app_icon: mode,
                    read_along_step: 3,
                };
                let bytes = serde_json::to_vec(&AppearanceRecord::from(settings)).unwrap();
                let restored: AppearanceRecord = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(restored.settings(), settings);
            }
        }
        let unknown = AppearanceRecord {
            mode: "unknown".into(),
            accent: "unknown".into(),
            tint_with_cover: true,
            app_icon: None,
            read_along_step: None,
        };
        assert_eq!(unknown.settings().mode, ui::theme::Mode::Light);
        assert_eq!(unknown.settings().app_icon, ui::theme::Mode::Automatic);
        assert_eq!(
            unknown.settings().read_along_step,
            ui::theme::READ_ALONG_DEFAULT_STEP
        );
        assert_eq!(unknown.settings().accent, ui::theme::AccentTheme::Ember);
    }

    #[test]
    fn superseded_appearance_writes_are_skipped() {
        let old = register_appearance_write();
        let _new = register_appearance_write();
        assert!(save_appearance(ui::ThemeSettings::default(), old).is_ok());
    }

    #[test]
    fn obsolete_position_writes_are_discarded_before_accessing_credentials() {
        let asin = audible::Asin::new("TESTPOSITION").unwrap();
        let old = register_save(&asin);
        let _new = register_save(&asin);
        assert!(save_position(&"a".repeat(64), &asin, 1200, old).is_ok());
    }
}

pub fn chapters(key: &str, asin: &audible::Asin) -> anyhow::Result<audible::models::ChapterInfo> {
    let path = account_directory(key)?
        .join("Chapters")
        .join(format!("{asin}.json"));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to load chapters"))?;
    match rt.block_on(client.chapters(asin)) {
        Ok(chapters) => {
            let _ = write_json(&path, &chapters);
            Ok(chapters)
        }
        Err(error) => std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| error.into()),
    }
}

static BOOKMARK_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn bookmark_path(key: &str, asin: &audible::Asin) -> anyhow::Result<std::path::PathBuf> {
    Ok(account_directory(key)?
        .join("Bookmarks")
        .join(format!("{asin}.json")))
}

fn read_bookmark_positions(path: &std::path::Path) -> anyhow::Result<Vec<u64>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn bookmarks(key: &str, asins: &[audible::Asin]) -> anyhow::Result<Vec<(audible::Asin, u64)>> {
    let _guard = BOOKMARK_SERIAL
        .lock()
        .map_err(|_| anyhow::anyhow!("Could not access bookmarks"))?;
    let mut result = Vec::new();
    for asin in asins {
        result.extend(
            read_bookmark_positions(&bookmark_path(key, asin)?)?
                .into_iter()
                .map(|position| (asin.clone(), position)),
        );
    }
    Ok(result)
}

pub fn update_bookmark(
    key: &str,
    asin: &audible::Asin,
    position: u64,
    add: bool,
) -> anyhow::Result<()> {
    let _guard = BOOKMARK_SERIAL
        .lock()
        .map_err(|_| anyhow::anyhow!("Could not access bookmarks"))?;
    let path = bookmark_path(key, asin)?;
    update_bookmark_file(&path, position, add)
}

fn update_bookmark_file(path: &std::path::Path, position: u64, add: bool) -> anyhow::Result<()> {
    let mut positions = read_bookmark_positions(path)?;
    if add {
        if !positions.contains(&position) {
            positions.push(position);
            positions.sort_unstable();
        }
    } else {
        positions.retain(|saved| *saved != position);
    }
    write_json(path, &positions)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AppearanceRecord {
    mode: String,
    accent: String,
    tint_with_cover: bool,
    /// Missing in files saved before the app icon setting existed.
    #[serde(default)]
    app_icon: Option<String>,
    /// Missing in files saved before read along had a text size.
    #[serde(default)]
    read_along_step: Option<u8>,
}

impl From<ui::ThemeSettings> for AppearanceRecord {
    fn from(settings: ui::ThemeSettings) -> Self {
        Self {
            mode: settings.mode.label().into(),
            accent: settings.accent.name().into(),
            tint_with_cover: settings.tint_with_cover,
            app_icon: Some(settings.app_icon.label().into()),
            read_along_step: Some(settings.read_along_step),
        }
    }
}

impl AppearanceRecord {
    fn settings(&self) -> ui::ThemeSettings {
        let mode = ui::theme::Mode::ALL
            .into_iter()
            .find(|mode| mode.label() == self.mode)
            .unwrap_or(ui::theme::Mode::Light);
        let accent = ui::theme::AccentTheme::ALL
            .into_iter()
            .find(|accent| accent.name() == self.accent)
            .unwrap_or_default();
        let app_icon = ui::theme::Mode::ALL
            .into_iter()
            .find(|mode| Some(mode.label()) == self.app_icon.as_deref())
            .unwrap_or_default();
        ui::ThemeSettings {
            mode,
            accent,
            tint_with_cover: self.tint_with_cover,
            app_icon,
            read_along_step: ui::theme::READ_ALONG_DEFAULT_STEP,
        }
        .with_read_along_step(self.read_along_step.map_or(0, |step| {
            step as i8 - ui::theme::READ_ALONG_DEFAULT_STEP as i8
        }))
    }
}

fn appearance_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::paths::support()?.join("appearance.json"))
}

/// Speed, volume and download quality, carried over between launches.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaybackPrefs {
    pub speed: Option<f32>,
    pub volume: Option<f32>,
    #[serde(default)]
    pub download_quality: DownloadQuality,
    /// Which speech model read along downloads where it uses Whisper.
    #[serde(default)]
    pub speech_model: crate::whisper::Model,
}

/// How good, and how big, new downloads are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DownloadQuality {
    /// Audible's highest bitrate, usually about 128 kbps: around 55 MB an hour.
    #[default]
    High,
    /// Audible's lower bitrate, about 64 kbps: half the size, fine for speech.
    Standard,
}

impl DownloadQuality {
    pub const ALL: [Self; 2] = [Self::High, Self::Standard];

    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Standard => "Standard",
        }
    }

    pub(crate) fn audible(self) -> audible::models::Quality {
        match self {
            Self::High => audible::models::Quality::High,
            Self::Standard => audible::models::Quality::Normal,
        }
    }
}

fn playback_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(appearance_path()?.with_file_name("playback.json"))
}

pub fn load_playback() -> PlaybackPrefs {
    playback_path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

static PLAYBACK_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn save_playback(prefs: &PlaybackPrefs) -> anyhow::Result<()> {
    let _guard = PLAYBACK_SERIAL
        .lock()
        .map_err(|_| anyhow::anyhow!("Could not save playback settings"))?;
    write_json(&playback_path()?, prefs)
}

pub fn load_appearance() -> ui::ThemeSettings {
    let fallback = ui::ThemeSettings {
        mode: ui::theme::Mode::Light,
        ..Default::default()
    };
    appearance_path()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<AppearanceRecord>(&bytes).ok())
        .map(|record| record.settings())
        .unwrap_or(fallback)
}

static APPEARANCE_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static APPEARANCE_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn register_appearance_write() -> u64 {
    APPEARANCE_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

pub fn save_appearance(settings: ui::ThemeSettings, version: u64) -> anyhow::Result<()> {
    let _guard = APPEARANCE_SERIAL
        .lock()
        .map_err(|_| anyhow::anyhow!("Could not save appearance"))?;
    if APPEARANCE_VERSION.load(std::sync::atomic::Ordering::SeqCst) != version {
        return Ok(());
    }
    write_json(&appearance_path()?, &AppearanceRecord::from(settings))
}

pub struct ExploreFeed {
    pub genres: Vec<audible::models::Category>,
    pub new_releases: Vec<audible::models::Product>,
    pub warning: Option<String>,
}

pub fn explore() -> anyhow::Result<ExploreFeed> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to explore Audible"))?;
    let genres = rt.block_on(client.categories());
    let query = audible::api::catalog::CatalogQuery {
        sort: audible::api::catalog::CatalogSort::NewestFirst,
        ..Default::default()
    };
    let releases = rt.block_on(client.search_catalog(&query));
    let warning = match (&genres, &releases) {
        (Err(_), Err(_)) => {
            Some("Explore is unavailable right now. Search is still available.".into())
        }
        (Err(_), _) => Some("Genres are unavailable right now.".into()),
        (_, Err(_)) => Some("New releases are unavailable right now.".into()),
        _ => None,
    };
    Ok(ExploreFeed {
        genres: genres.unwrap_or_default(),
        new_releases: releases.map(|page| page.products).unwrap_or_default(),
        warning,
    })
}

pub fn browse_genre(
    id: audible::models::CategoryId,
) -> anyhow::Result<Vec<audible::models::Product>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to explore Audible"))?;
    Ok(rt
        .block_on(client.search_catalog(&audible::api::catalog::CatalogQuery::category(id)))?
        .products)
}
