//! Account and library operations, run away from GPUI's main thread.

use audible::{
    Audible, AudibleClient, auth::LoginSession, credentials::CredentialStore, models::LibraryItem,
};
use std::sync::Arc;

const PROFILE: &str = "default";

#[cfg(target_os = "macos")]
fn store() -> anyhow::Result<Arc<dyn CredentialStore>> {
    Ok(Arc::new(audible::credentials::KeychainStore::new()))
}
#[cfg(not(target_os = "macos"))]
fn store() -> anyhow::Result<Arc<dyn CredentialStore>> {
    anyhow::bail!("Account storage is currently supported on macOS only")
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Library {
    pub items: Vec<LibraryItem>,
    pub marketplace: String,
    pub account_key: String,
    #[serde(skip)]
    pub offline: bool,
}

fn account_directory(key: &str) -> anyhow::Result<std::path::PathBuf> {
    anyhow::ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid account identifier"
    );
    let home =
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
    Ok(std::path::PathBuf::from(home)
        .join("Library/Application Support/Better Audible/Accounts")
        .join(key))
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
            let library = Library {
                items,
                marketplace,
                account_key,
                offline: false,
            };
            let _ = write_json(&cache, &library);
            Ok(library)
        }
        Err(error) => {
            if let Ok(bytes) = std::fs::read(cache)
                && let Ok(mut library) = serde_json::from_slice::<Library>(&bytes)
                && library.account_key == account_key
            {
                library.offline = true;
                return Ok(library);
            }
            Err(error.into())
        }
    }
}

pub fn restore() -> anyhow::Result<Option<Library>> {
    if std::env::var_os("BETTER_AUDIBLE_SMOKE_TEST").is_some() {
        return Ok(None);
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
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

pub fn downloaded_path(key: &str, asin: &audible::Asin) -> Option<std::path::PathBuf> {
    let directory = account_directory(key).ok()?.join("Books");
    ["m4b", "m4a"]
        .iter()
        .map(|extension| directory.join(format!("{asin}.{extension}")))
        .find(|path| path.is_file())
}

pub fn download(
    key: &str,
    asin: audible::Asin,
    progress: Arc<std::sync::Mutex<audible::DownloadEvent>>,
) -> anyhow::Result<std::path::PathBuf> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();
    let client = Audible::from_store(store()?, PROFILE)?
        .ok_or_else(|| anyhow::anyhow!("Sign in to download books"))?;
    let mut request = audible::DownloadRequest::new(asin, account_directory(key)?.join("Books"));
    request.progress = Some(Arc::new(move |event| {
        if let Ok(mut current) = progress.lock() {
            *current = event;
        }
    }));
    Ok(rt.block_on(client.download_book(request))?.path)
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
    let home =
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
    let directory = std::path::PathBuf::from(home).join("Library/Caches/Better Audible/Covers");
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
        };
        assert_eq!(unknown.settings().mode, ui::theme::Mode::Light);
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
}

impl From<ui::ThemeSettings> for AppearanceRecord {
    fn from(settings: ui::ThemeSettings) -> Self {
        Self {
            mode: settings.mode.label().into(),
            accent: settings.accent.name().into(),
            tint_with_cover: settings.tint_with_cover,
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
        ui::ThemeSettings {
            mode,
            accent,
            tint_with_cover: self.tint_with_cover,
        }
    }
}

fn appearance_path() -> anyhow::Result<std::path::PathBuf> {
    let home =
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
    Ok(std::path::PathBuf::from(home)
        .join("Library/Application Support/Better Audible/appearance.json"))
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
