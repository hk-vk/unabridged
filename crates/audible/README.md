# audible

The backend library for Better Audible. It is pure Rust and async (tokio + reqwest/rustls), and it lets the app act as a registered **Audible for iPhone** device for the owner's own purchased titles. It has no GPUI dependency.

| Module | What it does |
| --- | --- |
| `auth` | OAuth 2 sign-in URL with PKCE, reading the redirect, device registration, token refresh, deregistration |
| `credentials` | `Credentials`, `Secret` (redacted, zeroed on drop), the `CredentialStore` trait, `KeychainStore` (macOS) and `MemoryStore` (tests) |
| `signing` | ADP request signing (`SHA256withRSA`) |
| `http` | The signed client, with retries (backoff plus `Retry-After`) and mapping of HTTP statuses to typed errors |
| `api::{library, catalog, content, position}` | Endpoint wrappers |
| `models` | serde types that tolerate nulls, and newtype IDs (`Asin`, `CategoryId`) |
| `download` | Resumable downloads (`.part` file plus `Range`) with progress callbacks |
| `drm` | AAX/AAXC decryption to M4B in pure Rust, with an ffmpeg fallback behind the `Decrypter` trait |
| `client` | The `AudibleClient` facade (object-safe) and the real `Audible` implementation |

## How the UI uses it

```rust
use std::sync::Arc;
use audible::{Audible, AudibleClient, DownloadRequest, Marketplace};
use audible::auth::{LoginSession, LOGIN_USER_AGENT};
use audible::credentials::KeychainStore;

let store = Arc::new(KeychainStore::new());

// App launch: restore the stored session if there is one.
let client: Option<Audible> = Audible::from_store(store.clone(), "default")?;

// Sign-in screen:
let session = LoginSession::new(Marketplace::UnitedStates);
let url = session.authorization_url()?;  // load this in a WKWebView
let cookies = session.initial_cookies(); // set these on the web view first (they reduce CAPTCHAs)
// Use LOGIN_USER_AGENT as the web view's user agent.
// On each navigation: if LoginSession::is_redirect(&nav_url) { stop loading; then: }
let client = Audible::sign_in(&session, &nav_url, store, "default").await?;

// From here on, hold it as `Arc<dyn AudibleClient>`. Tests and previews use a mock.
let items = client.library(&Default::default()).await?;
let book = client.download_book(DownloadRequest::new(asin, library_dir)).await?;
client.set_last_position(&asin, position_ms).await?;
```

* `DownloadRequest.progress` receives `DownloadEvent::{Licensing, Downloading, Decrypting}`. Forward the events to the UI thread through a channel.
* If a download future is dropped, `<file>.part` stays on disk. Calling `download_book` again resumes it.
* Branch on errors: `requires_sign_in()` means show sign-in. `Throttled` and `RateLimited` mean wait and retry (`is_transient()`). `UnsupportedDrm` means the title can't be downloaded (Widevine) and should say so. `LicenseDenied` means the title isn't owned or the license expired.
* Covers: `Product::cover_url(min_px)` picks from `product_images`. By default the library and catalog queries request sizes 500 and 1215.
* `AudibleClient` is only for the UI. Use `Audible` directly for `sign_out`, `activation_bytes` and `download_aax`.

## Manual verification

`examples/cli.rs` signs in through the system browser and stores credentials in the Keychain. It can list the library, download and decrypt one book, read a position and sign out (which also deregisters the device). It has not been run against a real account.

```sh
cargo run --manifest-path crates/audible/Cargo.toml --example cli -- login us
cargo run --manifest-path crates/audible/Cargo.toml --example cli -- list
cargo run --manifest-path crates/audible/Cargo.toml --example cli -- download <ASIN> ~/Audiobooks
cargo run --manifest-path crates/audible/Cargo.toml --example cli -- logout
```

## Protocol notes

The main source is mkb79's Python `audible` 0.13.1 (`auth.py`, `login.py`, `register.py`, `activation_bytes.py`, `aescipher.py`, `localization.py`, `_device.py`). The others are audible-cli (`models.py`, `downloader.py`), FFmpeg `libavformat/mov.c` (`mov_read_adrm`, `aax_filter`, `mov_aaxc_crypto`), AAXClean (`AaxFile.cs`, `AavdFilter.cs`) and Libation's release notes.

### Sign-in and registration

* **Authorization URL.** The URL is `https://www.amazon.<tld>/ap/signin` with 18 `openid.*` parameters, copied verbatim from `build_oauth_url`. `assoc_handle` is `amzn_audible_ios_<cc>`. `client_id` is `device:` followed by hex(`<serial>#A2CZJZGLK2JJVM`). The PKCE challenge uses S256 over a verifier made from 32 random bytes in base64url.
* **Redirect.** The redirect goes to `https://www.amazon.<tld>/ap/maplanding?...openid.oa2.authorization_code=...`. That page returns 404 by design.
* **Registration.** `POST https://api.amazon.<tld>/auth/register`. The library asks for these token types: `bearer`, `mac_dms`, `website_cookies` and `store_authentication_cookie`. The response provides:
  * the RSA private key (`device_private_key`) and the `adp_token`;
  * the access and refresh tokens;
  * `device_info` and `customer_info`.
* **Device profile.** The library uses the iPhone profile from `_device.py`: app 4.60, build 46000856, iOS 26.2.
* **Serial number.** The serial is 40 random ASCII alphanumerics, as `build_device_serial` produces. `DeviceSerial` rejects anything else, and the serial is hex-encoded exactly once (in `client_id`). The library also checks that the serial echoed in the registration response matches the one sent.
  * Background: Libation 14.2.0 fixed "License Denied / CustomerThrottled" errors. Its release notes say the cause was "registering Android devices with a serial number twice the expected length".
  * Libation issue #2138 does not describe the serial bug. It is about switching an iPhone registration to Android to get Widevine. The fix is from the 14.2.0 release notes.
* **Token refresh.** `POST /auth/token` (form-encoded) returns a new access token that lasts 60 minutes. `Audible::access_token()` refreshes when the token has less than 5 minutes left. Refreshes are serialized behind a mutex, and the new token is persisted.
* **Deregistration.** `POST /auth/deregister` with the `x-amz-access-token` header.

### Request signing

* **Signed string.** `METHOD\npath?query\n<date>\n<body>\n<adp_token>`, signed with RSASSA-PKCS1-v1_5 and SHA-256.
* **Headers.** `x-adp-token`, `x-adp-alg: SHA256withRSA:1.0` and `x-adp-signature: <b64>:<date>`.
* **Date format.** The date matches what Python's `datetime.now(UTC).isoformat("T") + "Z"` produces, for example `2026-11-15T12:34:56.789012+00:00Z`. The `+00:00Z` ending is odd but known to work, so the library copies it exactly.
* **Re-signing.** Every retry is signed again with a fresh date.

### Endpoints (`https://api.audible.<tld>`)

| Use | Request |
| --- | --- |
| Library | `GET /1.0/library?num_results≤1000&page=N&response_groups=…&image_sizes=…&sort_by=-PurchaseDate`. The total is in the `Total-Count` response header. Paging stops when the total is reached or a short page arrives. |
| Item | `GET /1.0/library/{asin}` and `GET /1.0/catalog/products/{asin}` |
| Search and browse | `GET /1.0/catalog/products?keywords|category_id&num_results≤50&page&products_sort_by` |
| Genres | `GET /1.0/catalog/categories?root=Genres&categories_num_levels=2` |
| Chapters | `GET /1.0/content/{asin}/metadata?response_groups=chapter_info,content_reference,last_position_heard&chapter_titles_type=Tree` |
| Positions | `GET /1.0/annotations/lastpositions?asins=a,b` (the library sends 25 ASINs per request) and `PUT /1.0/lastpositions/{asin}` with body `{acr, asin, position_ms}` |
| License | `POST /1.0/content/{asin}/licenserequest` (details below) |
| Activation bytes | Signed `GET https://www.audible.com/license/token?player_manuf=Audible,iPhone&action=register&player_model=iPhone`. The library takes the last 0x238 bytes of the response (8 lines of 70 bytes plus `\n`). The first 4 bytes, read little-endian, are the activation bytes. |
| AAX download | Signed `GET https://www.audible.<tld>/library/download?asin=…&codec=<enhanced_codec>` |

* **License request body.** `{"supported_drm_types":["Mpeg","Adrm"],"quality":"High","consumption_type":"Download",...}`.
* **License request headers.** The library sends the iOS app's headers, as audible-cli does: `X-ADP-SW`, `X-ADP-Transport`, `X-ADP-LTO`, `X-Device-Type-Id`, `device_idiom` and a random `X-Amzn-RequestId`.
* **License result.**
  * `status_code != Granted`: if the denial text mentions throttling, the result is `Error::Throttled`. If it mentions Widevine or DRM, the result is `Error::UnsupportedDrm`. Otherwise it is `Error::LicenseDenied`.
  * `drm_type == Adrm`: the library decrypts the voucher (below).
  * `drm_type == Mpeg`: the file is plain M4A and needs no decryption.
  * Any other `drm_type`: `Error::UnsupportedDrm { drm_type }`. Widevine is never requested or implemented.
* **AAXC voucher.** The library computes `sha256(device_type ‖ serial ‖ customer_id ‖ asin)`. The first 16 bytes are the AES key and the last 16 are the IV. It base64-decodes the voucher, decrypts it with AES-128-CBC (no padding) and strips NULs. The result is JSON whose `key` and `iv` fields are hex.

### Decryption (AAX and AAXC)

* **Encryption scheme.** Audio sits in an MP4 whose sample entry is named `aavd`. Each sample is encrypted with AES-128-CBC, using the file key and the file IV. The IV is reset for every sample, and the trailing `len % 16` bytes are left unencrypted. AAXClean decrypts only the audio track, and so does this library.
* **AAX keys.** The file key is derived from `adrm` (blob at offset 8..64, checksum at 68..88) with FFmpeg's fixed key `77214d4b196a87cd520045fd20a51d67`:
  1. `ik = sha1(fixed ‖ ab)`
  2. `iiv = sha1(fixed ‖ ik ‖ ab)`
  3. Check that `sha1(ik[:16] ‖ iiv[:16])` equals the stored checksum.
  4. Decrypt the first 48 bytes of the blob.
  5. Check that bytes 0..4, reversed, equal the activation bytes.
  6. The file key is bytes 8..24. The file IV is `sha1(out[26..42] ‖ key ‖ fixed)[:16]`.
* **AAXC keys.** The key and IV come straight from the voucher.
* **Output.**
  * The file is cloned (`std::fs::copy` uses APFS clonefile on macOS), and the samples are decrypted in place.
  * These patches keep the same size: `aavd`→`mp4a`, `adrm`/`aabd`→`free`, and the `aax`/`aaxc` brands→`M4B `. No offsets move.
  * Chapters, the cover and metadata are left as they are.
* **ffmpeg fallback.** `FfmpegDecrypter` is the fallback. It runs `ffmpeg -activation_bytes …` (or `-audible_key/-audible_iv`) followed by `-c copy`. FFmpeg turns on AAXC mode only when the major brand is `aaxc`, which real AAXC files have.

## What is tested, and what is only taken from the references

All tests run offline. They use wiremock and temp dirs, and none of them touch the network. The Keychain round-trip test is `#[ignore]`d because it writes to the real login keychain. It passed when run manually.

**Checked against independent implementations:**
* **Signing:** known-answer tests against `openssl dgst -sha256 -sign`. A wiremock test also checks the signature against what actually went over the wire, including query encoding.
* **PKCE:** the RFC 7636 test vector.
* **AES-CBC:** the NIST SP 800-38A vectors.
* **AAXC voucher:** the fixture was encrypted with `openssl enc` using a key from Python `hashlib`.
* **AAX key derivation:** a known-answer `adrm` blob produced with Python and openssl.
* **AAX/AAXC files:** synthetic files built from a real FFmpeg-encoded AAC fixture. The native decrypter and the installed FFmpeg's own AAX/AAXC decryption produce the same audio packets (md5) as the original. Those two tests are skipped when ffmpeg is missing.

**Behaviour tested against mocks:**
* Pagination, including a retry in the middle of paging.
* Retries on 429/503, with no retry of POST on 500, and the status-to-error mapping.
* Resuming, restarting, 416 handling and truncated transfers in the downloader.
* License results: throttled, Widevine, denied and Mpeg.
* Position sync with acr lookup and caching.
* Token refresh that runs once and is persisted.
* Sign-in, restore, sign-out, and a full `download_book` from license to M4B.

**Only taken from the references, never checked against the live service:**
* The shapes of the real server responses. The fixtures were written by hand from what the Python and CLI code reads.
* Exact response-group availability.
* The `catalog/categories` response shape.
* The limit on how many ASINs one `lastpositions` request accepts.
* Whether `Total-Count` is always present.

## Known risks

* **API drift.** Amazon changes the login pages, the app versions it accepts and the response fields without notice. The device profile constants in `auth/device.rs` may need bumping when the Python library bumps them. The models make every field optional to survive changes in response shape.
* **Throttling.** Audible throttles license requests (`CustomerThrottled`). If one account registers many devices or registrations look malformed, licenses get denied. Register once and reuse the stored device rather than signing in again. Use `sign_out(true)` so old devices are deregistered. On `Error::Throttled`, wait and retry later instead of in a loop.
* **Widevine.** Some titles (for example Dolby Atmos/spatial audio, or the newer DASH-only catalogue) may only be served with Widevine, and the iPhone profile cannot get Widevine licenses. Those titles come back as `Error::UnsupportedDrm`, and supporting them is out of scope.
* **AAXC files from the CDN.** The library expects them to be non-fragmented MP4 with an `aavd` sample entry, as AAXClean and FFmpeg handle them. A fragmented file returns `Error::InvalidMedia`; ffmpeg would not help with one either.
* **xHE-AAC/USAC and E-AC-3 codecs.** These are decrypted the same way, but playback depends on AVFoundation.
* **Player compatibility.** The output keeps `free` boxes inside the `mp4a` sample entry, which is valid ISO-BMFF. AAXClean also clears an `esds` flag (`DependsOnCoreCoder`) that it says stops cover art loading on Android. This library does not change that flag, and it is not needed for macOS playback, but that hasn't been checked.
* **Key material passed to ffmpeg.** `FfmpegDecrypter` puts keys on the command line, where other local processes can see them while it runs.
