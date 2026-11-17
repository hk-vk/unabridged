//! The [`AudibleClient`] facade the UI talks to, and its real implementation
//! [`Audible`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Duration;
use reqwest::Method;
use tracing::{info, warn};

use crate::api::catalog::{self, CatalogQuery};
use crate::api::content::{self, ContentDrm, License};
use crate::api::library::{self, LibraryQuery};
use crate::api::position;
use crate::auth::{self, LoginSession};
use crate::credentials::{CredentialStore, Credentials, Secret};
use crate::download::{Downloader, Progress};
use crate::drm::{ActivationBytes, Decrypter, DecryptionKey, NativeDecrypter};
use crate::error::{Error, Result};
use crate::http::{Host, HttpClient, RetryPolicy};
use crate::locale::{Endpoints, Marketplace};
use crate::models::{
    Asin, CatalogPage, Category, ChapterInfo, ContentMetadata, LibraryItem, LibraryPage, Product,
    Quality, TitlePosition,
};
use crate::signing::AdpSigner;

/// Progress of [`AudibleClient::download_book`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEvent {
    /// Requesting the license.
    Licensing,
    /// Downloading the (encrypted) file.
    Downloading(Progress),
    /// Decrypting to M4B.
    Decrypting(Progress),
}

/// Callback for [`DownloadEvent`]s. Keep it cheap (forward to a channel).
pub type DownloadCallback = Arc<dyn Fn(DownloadEvent) + Send + Sync>;

/// What to download and where.
#[derive(Clone)]
pub struct DownloadRequest {
    /// Title to download.
    pub asin: Asin,
    /// Requested quality.
    pub quality: Quality,
    /// Directory for the output (created if missing).
    pub directory: PathBuf,
    /// Keep the encrypted `.aaxc` next to the `.m4b`.
    pub keep_encrypted: bool,
    /// Progress callback.
    pub progress: Option<DownloadCallback>,
}

impl std::fmt::Debug for DownloadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadRequest")
            .field("asin", &self.asin)
            .field("quality", &self.quality)
            .field("directory", &self.directory)
            .field("keep_encrypted", &self.keep_encrypted)
            .finish_non_exhaustive()
    }
}

impl DownloadRequest {
    /// Download `asin` in high quality into `directory`.
    pub fn new(asin: Asin, directory: impl Into<PathBuf>) -> Self {
        Self {
            asin,
            quality: Quality::High,
            directory: directory.into(),
            keep_encrypted: false,
            progress: None,
        }
    }

    fn emit(&self, event: DownloadEvent) {
        if let Some(cb) = &self.progress {
            cb(event);
        }
    }
}

/// A finished download.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadedBook {
    /// Playable file (`.m4b`, or `.m4a` for DRM-free content).
    pub path: PathBuf,
    /// Chapters from the license, when returned.
    pub chapters: Option<ChapterInfo>,
    /// Content reference id, needed for position sync.
    pub acr: Option<String>,
}

/// Everything the UI needs from Audible. Implemented by [`Audible`]; mock it
/// in UI tests and previews.
#[async_trait]
pub trait AudibleClient: Send + Sync {
    /// Marketplace of the signed-in account.
    fn marketplace(&self) -> Marketplace;

    /// One library page (1-based).
    async fn library_page(&self, query: &LibraryQuery, page: u32) -> Result<LibraryPage>;

    /// The whole library.
    async fn library(&self, query: &LibraryQuery) -> Result<Vec<LibraryItem>>;

    /// One owned title with full details.
    async fn library_item(&self, asin: &Asin) -> Result<LibraryItem>;

    /// Chapters, content reference and server position.
    async fn content_metadata(&self, asin: &Asin) -> Result<ContentMetadata>;

    /// Chapter table.
    async fn chapters(&self, asin: &Asin) -> Result<ChapterInfo>;

    /// Server-side positions for several titles.
    async fn last_positions(&self, asins: &[Asin]) -> Result<Vec<TitlePosition>>;

    /// Write the server-side position.
    async fn set_last_position(&self, asin: &Asin, position_ms: u64) -> Result<()>;

    /// Catalog search/browse for Explore.
    async fn search_catalog(&self, query: &CatalogQuery) -> Result<CatalogPage>;

    /// Catalog details for any title.
    async fn catalog_product(&self, asin: &Asin) -> Result<Product>;

    /// Top-level genres (with one level of children).
    async fn categories(&self) -> Result<Vec<Category>>;

    /// Request a download license.
    async fn license(&self, asin: &Asin, quality: Quality) -> Result<License>;

    /// License, download and decrypt a title to a playable file.
    async fn download_book(&self, request: DownloadRequest) -> Result<DownloadedBook>;
}

/// Where and under which profile credentials are persisted.
#[derive(Clone)]
struct Persistence {
    store: Arc<dyn CredentialStore>,
    profile: String,
}

/// Builder for [`Audible`].
pub struct AudibleBuilder {
    credentials: Credentials,
    endpoints: Option<Endpoints>,
    http: Option<reqwest::Client>,
    retry: RetryPolicy,
    decrypter: Arc<dyn Decrypter>,
    persistence: Option<Persistence>,
}

impl AudibleBuilder {
    /// Override endpoints (tests, proxies).
    pub fn endpoints(mut self, endpoints: Endpoints) -> Self {
        self.endpoints = Some(endpoints);
        self
    }

    /// Share an existing reqwest client.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// Retry policy for API calls and downloads.
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Use a different decrypter (e.g. [`crate::drm::FfmpegDecrypter`]).
    pub fn decrypter(mut self, decrypter: Arc<dyn Decrypter>) -> Self {
        self.decrypter = decrypter;
        self
    }

    /// Persist refreshed tokens and activation bytes to `store`.
    pub fn store(mut self, store: Arc<dyn CredentialStore>, profile: impl Into<String>) -> Self {
        self.persistence = Some(Persistence {
            store,
            profile: profile.into(),
        });
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<Audible> {
        let signer = AdpSigner::new(
            &self.credentials.adp_token,
            &self.credentials.device_private_key,
        )?;
        let endpoints = self
            .endpoints
            .unwrap_or_else(|| self.credentials.marketplace.endpoints());
        let inner = match self.http {
            Some(c) => c,
            None => default_http_client()?,
        };
        Ok(Audible {
            marketplace: self.credentials.marketplace,
            http: HttpClient::new(inner.clone(), endpoints, signer).with_retry(self.retry),
            downloader: Downloader::new(inner).with_retry(RetryPolicy {
                max_attempts: self.retry.max_attempts.max(5),
                ..self.retry
            }),
            decrypter: self.decrypter,
            credentials: Arc::new(tokio::sync::Mutex::new(self.credentials)),
            persistence: self.persistence,
            acr_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

fn default_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(60))
        .gzip(true)
        .build()
        .map_err(Error::from)
}

/// The real Audible client: a registered device plus transport. Cheap to
/// clone; clones share credentials and caches.
#[derive(Clone)]
pub struct Audible {
    marketplace: Marketplace,
    http: HttpClient,
    downloader: Downloader,
    decrypter: Arc<dyn Decrypter>,
    credentials: Arc<tokio::sync::Mutex<Credentials>>,
    persistence: Option<Persistence>,
    acr_cache: Arc<Mutex<HashMap<Asin, String>>>,
}

impl std::fmt::Debug for Audible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audible")
            .field("endpoints", self.http.endpoints())
            .finish_non_exhaustive()
    }
}

impl Audible {
    /// Start building a client from stored or fresh credentials.
    pub fn builder(credentials: Credentials) -> AudibleBuilder {
        AudibleBuilder {
            credentials,
            endpoints: None,
            http: None,
            retry: RetryPolicy::default(),
            decrypter: Arc::new(NativeDecrypter),
            persistence: None,
        }
    }

    /// Restore a signed-in client from `store`, or `None` if `profile` has
    /// never signed in.
    pub fn from_store(store: Arc<dyn CredentialStore>, profile: &str) -> Result<Option<Self>> {
        match store.load(profile)? {
            Some(creds) => Self::builder(creds).store(store, profile).build().map(Some),
            None => Ok(None),
        }
    }

    /// Finish a sign-in: register the device from the redirect URL, save the
    /// credentials to `store` under `profile`, and return a ready client.
    pub async fn sign_in(
        session: &LoginSession,
        redirect_url: &str,
        store: Arc<dyn CredentialStore>,
        profile: &str,
    ) -> Result<Self> {
        let http = default_http_client()?;
        let creds = auth::register_device(&http, session, redirect_url).await?;
        store.save(profile, &creds)?;
        Self::builder(creds)
            .endpoints(session.endpoints().clone())
            .http_client(http)
            .store(store, profile)
            .build()
    }

    /// A snapshot of the current credentials.
    pub async fn credentials(&self) -> Credentials {
        self.credentials.lock().await.clone()
    }

    /// The low-level signed HTTP client, for endpoints not wrapped here.
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    /// A valid bearer access token, refreshed (and persisted) when it
    /// expires within five minutes. Concurrent callers share one refresh.
    pub async fn access_token(&self) -> Result<Secret> {
        let mut creds = self.credentials.lock().await;
        if creds.access_token_expires_within(Duration::minutes(5)) {
            let fresh = auth::refresh_access_token(
                self.http.reqwest(),
                self.http.endpoints(),
                &creds.refresh_token,
            )
            .await?;
            creds.access_token = fresh.token;
            creds.access_token_expires = fresh.expires;
            self.persist(&creds);
        }
        Ok(creds.access_token.clone())
    }

    /// Activation bytes for AAX files: cached in the credentials, fetched
    /// and persisted on first use.
    pub async fn activation_bytes(&self) -> Result<ActivationBytes> {
        if let Some(ab) = self.credentials.lock().await.activation_bytes.as_ref() {
            return ab.expose().parse();
        }
        let ab = content::activation_bytes(&self.http).await?;
        let mut creds = self.credentials.lock().await;
        creds.activation_bytes = Some(Secret::new(ab.to_hex()));
        self.persist(&creds);
        Ok(ab)
    }

    /// Sign out: optionally deregister the device with Amazon (recommended,
    /// keeps the account's device list clean), then delete stored
    /// credentials.
    pub async fn sign_out(&self, deregister: bool) -> Result<()> {
        if deregister {
            let token = self.access_token().await?;
            auth::deregister_device(self.http.reqwest(), self.http.endpoints(), &token, false)
                .await?;
        }
        if let Some(p) = &self.persistence {
            p.store.delete(&p.profile)?;
        }
        info!("signed out");
        Ok(())
    }

    /// Download an AAX file (older format, keyed by activation bytes) using
    /// `codec` from [`Product::available_codecs`] (`enhanced_codec`, e.g.
    /// `LC_128_44100_stereo`), then decrypt it to `<directory>/<asin>.m4b`.
    pub async fn download_aax(
        &self,
        asin: &Asin,
        codec: &str,
        request: &DownloadRequest,
    ) -> Result<PathBuf> {
        let ab = self.activation_bytes().await?;
        let url = self.http.url(
            Host::AudibleWww,
            "/library/download",
            &[("asin", asin.to_string()), ("codec", codec.to_owned())],
        )?;
        let headers = self.http.sign(&Method::GET, &url, b"");
        let encrypted = request.directory.join(format!("{asin}.aax"));
        self.fetch(url.as_str(), &headers, &encrypted, request)
            .await?;
        let output = request.directory.join(format!("{asin}.m4b"));
        self.decrypt(&encrypted, &output, &DecryptionKey::Aax(ab), request)
            .await?;
        Ok(output)
    }

    async fn fetch(
        &self,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        dest: &Path,
        request: &DownloadRequest,
    ) -> Result<u64> {
        let progress = request.progress.clone().map(|cb| {
            Arc::new(move |p: Progress| cb(DownloadEvent::Downloading(p)))
                as crate::download::ProgressCallback
        });
        self.downloader.download(url, headers, dest, progress).await
    }

    async fn decrypt(
        &self,
        input: &Path,
        output: &Path,
        key: &DecryptionKey,
        request: &DownloadRequest,
    ) -> Result<()> {
        request.emit(DownloadEvent::Decrypting(Progress {
            downloaded: 0,
            total: None,
        }));
        let progress = request.progress.clone().map(|cb| {
            Arc::new(move |p: Progress| cb(DownloadEvent::Decrypting(p)))
                as crate::download::ProgressCallback
        });
        // Never publish a partly decrypted file under its playable name. The
        // temporary file stays on the same filesystem for an atomic rename.
        static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stem = output
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio");
        let temporary = output.with_file_name(format!(
            ".{stem}.{}.{sequence}.partial.m4b",
            std::process::id()
        ));
        if let Err(error) = self
            .decrypter
            .decrypt(input, &temporary, key, progress)
            .await
        {
            tokio::fs::remove_file(&temporary).await.ok();
            return Err(error);
        }
        if let Err(error) = tokio::fs::rename(&temporary, output).await {
            tokio::fs::remove_file(&temporary).await.ok();
            return Err(error.into());
        }
        if !request.keep_encrypted {
            tokio::fs::remove_file(input).await.ok();
        }
        Ok(())
    }

    fn persist(&self, creds: &Credentials) {
        if let Some(p) = &self.persistence
            && let Err(e) = p.store.save(&p.profile, creds)
        {
            // Not fatal: the in-memory session keeps working.
            warn!(error = %e, "could not persist refreshed credentials");
        }
    }

    fn remember_acr(&self, asin: &Asin, acr: Option<&str>) {
        if let (Some(acr), Ok(mut cache)) = (acr, self.acr_cache.lock()) {
            cache.insert(asin.clone(), acr.to_owned());
        }
    }

    fn cached_acr(&self, asin: &Asin) -> Option<String> {
        self.acr_cache.lock().ok()?.get(asin).cloned()
    }
}

#[async_trait]
impl AudibleClient for Audible {
    fn marketplace(&self) -> Marketplace {
        self.marketplace
    }

    async fn library_page(&self, query: &LibraryQuery, page: u32) -> Result<LibraryPage> {
        library::page(&self.http, query, page).await
    }

    async fn library(&self, query: &LibraryQuery) -> Result<Vec<LibraryItem>> {
        library::all(&self.http, query, |_| {}).await
    }

    async fn library_item(&self, asin: &Asin) -> Result<LibraryItem> {
        library::item(&self.http, asin, &[]).await
    }

    async fn content_metadata(&self, asin: &Asin) -> Result<ContentMetadata> {
        let meta = content::metadata(&self.http, asin, Quality::High).await?;
        self.remember_acr(
            asin,
            meta.content_reference.as_ref().map(|c| c.acr.as_str()),
        );
        Ok(meta)
    }

    async fn chapters(&self, asin: &Asin) -> Result<ChapterInfo> {
        self.content_metadata(asin)
            .await?
            .chapter_info
            .ok_or_else(|| Error::NotFound {
                message: format!("no chapter info for {asin}"),
            })
    }

    async fn last_positions(&self, asins: &[Asin]) -> Result<Vec<TitlePosition>> {
        position::get(&self.http, asins).await
    }

    async fn set_last_position(&self, asin: &Asin, position_ms: u64) -> Result<()> {
        let acr = match self.cached_acr(asin) {
            Some(acr) => acr,
            None => self
                .content_metadata(asin)
                .await?
                .content_reference
                .map(|c| c.acr)
                .ok_or(Error::MissingCredential(
                    "content reference (acr) for position sync",
                ))?,
        };
        position::set(&self.http, asin, &acr, position_ms).await
    }

    async fn search_catalog(&self, query: &CatalogQuery) -> Result<CatalogPage> {
        catalog::search(&self.http, query).await
    }

    async fn catalog_product(&self, asin: &Asin) -> Result<Product> {
        catalog::product(&self.http, asin).await
    }

    async fn categories(&self) -> Result<Vec<Category>> {
        catalog::categories(&self.http, "Genres", 2).await
    }

    async fn license(&self, asin: &Asin, quality: Quality) -> Result<License> {
        let creds = self.credentials.lock().await.clone();
        let lic = content::license(&self.http, &creds, asin, quality).await?;
        self.remember_acr(asin, lic.acr());
        Ok(lic)
    }

    async fn download_book(&self, request: DownloadRequest) -> Result<DownloadedBook> {
        request.emit(DownloadEvent::Licensing);
        let lic = self.license(&request.asin, request.quality).await?;
        let asin = &request.asin;
        let no_headers = reqwest::header::HeaderMap::new();
        let path = match &lic.drm {
            ContentDrm::None => {
                let dest = request.directory.join(format!("{asin}.m4a"));
                self.fetch(lic.download_url.expose(), &no_headers, &dest, &request)
                    .await?;
                dest
            }
            ContentDrm::Aaxc(key) => {
                let encrypted = request.directory.join(format!("{asin}.aaxc"));
                self.fetch(lic.download_url.expose(), &no_headers, &encrypted, &request)
                    .await?;
                let output = request.directory.join(format!("{asin}.m4b"));
                self.decrypt(&encrypted, &output, key, &request).await?;
                output
            }
        };
        Ok(DownloadedBook {
            path,
            acr: lic.acr().map(str::to_owned),
            chapters: lic.chapter_info,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{CodeVerifier, DeviceSerial};
    use crate::credentials::MemoryStore;
    use std::sync::atomic::{AtomicBool, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SERIAL: &str = "Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0f";

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn fast_retry() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 2,
            base_delay: std::time::Duration::from_millis(1),
            max_delay: std::time::Duration::from_millis(5),
        }
    }

    async fn signed_in(server: &MockServer, store: Arc<dyn CredentialStore>) -> Audible {
        let key = fixture("test_rsa_key.pem");
        let body = fixture("register_response.json")
            .replace("PRIVATE_KEY_PLACEHOLDER", &key.replace('\n', "\\n"));
        Mock::given(method("POST"))
            .and(path("/auth/register"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(server)
            .await;
        let session = LoginSession::with_parts(
            Marketplace::UnitedStates,
            Endpoints::all(&server.uri()),
            DeviceSerial::new(SERIAL).unwrap(),
            CodeVerifier::from_string("v"),
        );
        let redirect = "https://www.amazon.com/ap/maplanding?openid.oa2.authorization_code=AN1";
        let client = Audible::sign_in(&session, redirect, store, "default")
            .await
            .unwrap();
        let creds = client.credentials().await;
        Audible::builder(creds)
            .endpoints(Endpoints::all(&server.uri()))
            .retry(fast_retry())
            .store(client.persistence.clone().unwrap().store, "default")
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn sign_in_persists_and_restores() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryStore::new());
        let client = signed_in(&server, store.clone()).await;
        assert_eq!(client.marketplace(), Marketplace::UnitedStates);
        let restored = Audible::from_store(store.clone(), "default")
            .unwrap()
            .unwrap();
        assert_eq!(restored.credentials().await, client.credentials().await);
        assert!(Audible::from_store(store, "other").unwrap().is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sign_in_persists_to_a_private_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Credentials");
        let store = Arc::new(crate::credentials::FileStore::new(&folder));
        let client = signed_in(&server, store.clone()).await;
        let restored = Audible::from_store(store.clone(), "default")
            .unwrap()
            .unwrap();
        assert_eq!(restored.credentials().await, client.credentials().await);
        let mode = |path: &std::path::Path| {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode(&folder), 0o700);
        assert_eq!(mode(&folder.join("default.json")), 0o600);
        store.delete("default").unwrap();
        assert!(Audible::from_store(store, "default").unwrap().is_none());
    }

    #[tokio::test]
    async fn download_book_licenses_downloads_and_decrypts() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryStore::new());
        let client = signed_in(&server, store).await;

        let license =
            fixture("license_aaxc.json").replace("https://dcdn.audible.example", &server.uri());
        Mock::given(method("POST"))
            .and(path("/1.0/content/B002V5D7RU/licenserequest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(license))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        // Encrypted with the key/iv inside the voucher fixture.
        let key = crate::crypto::hex16("0123456789abcdef0123456789abcdef").unwrap();
        let iv = crate::crypto::hex16("fedcba9876543210fedcba9876543210").unwrap();
        let encrypted = crate::drm::convert::tests::make_encrypted(dir.path(), None, &key, &iv);
        Mock::given(method("GET"))
            .and(path("/B002V5D7RU.aaxc"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(encrypted))
            .mount(&server)
            .await;

        let saw_decrypt = Arc::new(AtomicBool::new(false));
        let flag = saw_decrypt.clone();
        let mut request =
            DownloadRequest::new(Asin::new("B002V5D7RU").unwrap(), dir.path().join("books"));
        request.progress = Some(Arc::new(move |e| {
            if matches!(e, DownloadEvent::Decrypting(_)) {
                flag.store(true, Ordering::Relaxed);
            }
        }));
        let book = client.download_book(request).await.unwrap();

        assert_eq!(book.path, dir.path().join("books/B002V5D7RU.m4b"));
        assert_eq!(book.acr.as_deref(), Some("CR!7XQ4B3JH9V0KQ8WZ0Y0RNX6MCB1E"));
        assert_eq!(book.chapters.unwrap().chapters.len(), 2);
        assert!(saw_decrypt.load(Ordering::Relaxed));
        assert!(!dir.path().join("books/B002V5D7RU.aaxc").exists());
        let mut out = std::fs::read(&book.path).unwrap();
        out[8..12].copy_from_slice(b"M4A ");
        let original = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sine.m4a"
        ))
        .unwrap();
        assert_eq!(out, original);
    }

    #[tokio::test]
    async fn failed_decryption_never_publishes_a_partial_book() {
        struct FailingDecrypter;
        #[async_trait]
        impl Decrypter for FailingDecrypter {
            async fn decrypt(
                &self,
                _input: &Path,
                output: &Path,
                _key: &DecryptionKey,
                _progress: Option<crate::download::ProgressCallback>,
            ) -> Result<()> {
                tokio::fs::write(output, b"partial").await?;
                Err(Error::InvalidMedia("test failure".into()))
            }
        }
        let server = MockServer::start().await;
        let mut client = signed_in(&server, Arc::new(MemoryStore::new())).await;
        client.decrypter = Arc::new(FailingDecrypter);
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("book.aaxc");
        let output = dir.path().join("book.m4b");
        tokio::fs::write(&input, b"encrypted").await.unwrap();
        tokio::fs::write(&output, b"existing").await.unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let mut request = DownloadRequest::new(Asin::new("TEST").unwrap(), dir.path());
        request.progress = Some(Arc::new(move |event| captured.lock().unwrap().push(event)));
        let key = DecryptionKey::Aaxc {
            key: [0; 16],
            iv: [0; 16],
        };
        assert!(
            client
                .decrypt(&input, &output, &key, &request)
                .await
                .is_err()
        );
        assert_eq!(tokio::fs::read(&output).await.unwrap(), b"existing");
        assert!(matches!(
            events.lock().unwrap().first(),
            Some(DownloadEvent::Decrypting(Progress {
                downloaded: 0,
                total: None
            }))
        ));
        assert!(input.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn position_write_looks_up_and_caches_acr() {
        let server = MockServer::start().await;
        let client = signed_in(&server, Arc::new(MemoryStore::new())).await;
        Mock::given(path("/1.0/content/B002V5D7RU/metadata"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(fixture("content_metadata.json")),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/1.0/lastpositions/B002V5D7RU"))
            .respond_with(ResponseTemplate::new(204))
            .expect(2)
            .mount(&server)
            .await;
        let asin = Asin::new("B002V5D7RU").unwrap();
        client.set_last_position(&asin, 1000).await.unwrap();
        client.set_last_position(&asin, 2000).await.unwrap();
    }

    #[tokio::test]
    async fn expired_access_token_is_refreshed_once_and_persisted() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryStore::new());
        let client = signed_in(&server, store.clone()).await;
        client.credentials.lock().await.access_token_expires = chrono::Utc::now();
        Mock::given(method("POST"))
            .and(path("/auth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_string(fixture("token_refresh.json")))
            .expect(1)
            .mount(&server)
            .await;
        let (a, b) = tokio::join!(client.access_token(), client.access_token());
        assert_eq!(a.unwrap().expose(), "Atna|REFRESHEDACCESSTOKEN");
        assert_eq!(b.unwrap().expose(), "Atna|REFRESHEDACCESSTOKEN");
        let stored = store.load("default").unwrap().unwrap();
        assert_eq!(stored.access_token.expose(), "Atna|REFRESHEDACCESSTOKEN");
    }

    #[tokio::test]
    async fn sign_out_deregisters_and_deletes() {
        let server = MockServer::start().await;
        let store = Arc::new(MemoryStore::new());
        let client = signed_in(&server, store.clone()).await;
        Mock::given(method("POST"))
            .and(path("/auth/deregister"))
            .and(wiremock::matchers::header(
                "x-amz-access-token",
                "Atna|EXAMPLEACCESSTOKEN",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"response":{"success":{}}}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        client.sign_out(true).await.unwrap();
        assert!(store.load("default").unwrap().is_none());
    }

    /// The facade is object-safe so the UI can hold `Arc<dyn AudibleClient>`.
    #[test]
    fn facade_is_object_safe() {
        fn takes(_: Option<Arc<dyn AudibleClient>>) {}
        takes(None);
    }
}
