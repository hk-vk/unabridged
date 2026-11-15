//! Content endpoints: chapters/metadata, download licenses (with AAXC voucher
//! decryption and DRM detection) and AAX activation bytes.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngCore;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use crate::credentials::{Credentials, Secret};
use crate::crypto::{BLOCK, aes128_cbc_decrypt_blocks, hex16};
use crate::drm::{ActivationBytes, DecryptionKey};
use crate::error::{Error, Result};
use crate::http::{Host, HttpClient};
use crate::models::{
    Asin, ChapterInfo, ContentMetadata, ContentMetadataResponse, ContentReference, LicenseResponse,
    Quality, RawContentLicense,
};

/// Fetch chapters, content reference and last position for a title.
pub async fn metadata(http: &HttpClient, asin: &Asin, quality: Quality) -> Result<ContentMetadata> {
    let body: ContentMetadataResponse = http
        .get_json(
            &format!("/1.0/content/{asin}/metadata"),
            &[
                (
                    "response_groups",
                    "chapter_info,content_reference,last_position_heard".to_owned(),
                ),
                ("quality", quality.as_str().to_owned()),
                ("drm_type", "Adrm".to_owned()),
                ("chapter_titles_type", "Tree".to_owned()),
            ],
        )
        .await?;
    Ok(body.content_metadata)
}

/// Fetch only the chapter table.
pub async fn chapters(http: &HttpClient, asin: &Asin) -> Result<ChapterInfo> {
    metadata(http, asin, Quality::High)
        .await?
        .chapter_info
        .ok_or_else(|| Error::NotFound {
            message: format!("no chapter info for {asin}"),
        })
}

/// DRM applied to a licensed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentDrm {
    /// Audible AAXC: decrypt with the key and IV from the voucher.
    Aaxc(DecryptionKey),
    /// Unencrypted MPEG/M4A (podcasts, some free titles).
    None,
}

/// A granted download license.
#[derive(Clone)]
pub struct License {
    /// Title ASIN.
    pub asin: Asin,
    /// Content reference; its `acr` is needed for position writes.
    pub content_reference: Option<ContentReference>,
    /// Chapters, when returned.
    pub chapter_info: Option<ChapterInfo>,
    /// DRM and key material.
    pub drm: ContentDrm,
    /// Pre-signed download URL (a bearer secret until it expires).
    pub download_url: Secret,
}

impl fmt::Debug for License {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("License")
            .field("asin", &self.asin)
            .field("content_reference", &self.content_reference)
            .field("drm", &self.drm)
            .field("download_url", &self.download_url)
            .finish_non_exhaustive()
    }
}

impl License {
    /// Audible content reference, if present.
    pub fn acr(&self) -> Option<&str> {
        self.content_reference.as_ref().map(|c| c.acr.as_str())
    }
}

/// Request a download license (`POST /1.0/content/{asin}/licenserequest`).
///
/// Only `Mpeg` and `Adrm` are offered as supported DRM types, as audible-cli
/// does. Titles Audible will only serve with other DRM (Widevine/DASH) yield
/// [`Error::UnsupportedDrm`]; throttling yields [`Error::Throttled`].
pub async fn license(
    http: &HttpClient,
    credentials: &Credentials,
    asin: &Asin,
    quality: Quality,
) -> Result<License> {
    let body = json!({
        "supported_drm_types": ["Mpeg", "Adrm"],
        "quality": quality.as_str(),
        "consumption_type": "Download",
        "chapter_titles_type": "Tree",
        "response_groups": "last_position_heard,pdf_url,content_reference,chapter_info",
    });
    let mut request_id = [0u8; 20];
    rand::rngs::OsRng.fill_bytes(&mut request_id);
    // Headers the iOS app sends (from audible-cli `LibraryItem.get_license`).
    let headers = [
        ("X-Amzn-RequestId", hex::encode_upper(request_id)),
        ("X-ADP-SW", "37801821".to_owned()),
        ("X-ADP-Transport", "WIFI".to_owned()),
        ("X-ADP-LTO", "120".to_owned()),
        ("X-Device-Type-Id", credentials.device_type.clone()),
        ("device_idiom", "phone".to_owned()),
    ];
    let resp: LicenseResponse = http
        .post_json(
            &format!("/1.0/content/{asin}/licenserequest"),
            &body,
            &headers,
        )
        .await?;
    evaluate_license(resp.content_license, credentials, asin)
}

/// Turn a raw license into a [`License`] or a typed error.
fn evaluate_license(
    raw: RawContentLicense,
    creds: &Credentials,
    requested: &Asin,
) -> Result<License> {
    let status = raw.status_code.as_deref().unwrap_or("");
    if status != "Granted" {
        return Err(denial_error(&raw));
    }
    let drm_type = raw.drm_type.as_deref().unwrap_or("Adrm");
    let asin = match raw.asin.as_deref() {
        Some(a) => Asin::new(a)?,
        None => requested.clone(),
    };
    let drm = match drm_type {
        "Adrm" => {
            let voucher = raw
                .license_response
                .as_deref()
                .ok_or_else(|| Error::LicenseDenied {
                    message: "granted Adrm license has no voucher".to_owned(),
                })?;
            ContentDrm::Aaxc(decrypt_voucher(voucher, creds, &asin)?)
        }
        "Mpeg" => ContentDrm::None,
        other => {
            warn!(%asin, drm = other, "title uses unsupported DRM");
            return Err(Error::UnsupportedDrm {
                drm_type: other.to_owned(),
            });
        }
    };
    let metadata = raw.content_metadata.unwrap_or(ContentMetadata {
        chapter_info: None,
        content_reference: None,
        last_position_heard: None,
        content_url: None,
    });
    let url = metadata
        .content_url
        .and_then(|u| u.offline_url)
        .ok_or_else(|| Error::LicenseDenied {
            message: "license has no download URL".to_owned(),
        })?;
    debug!(%asin, drm = drm_type, "license granted");
    Ok(License {
        asin,
        content_reference: metadata.content_reference,
        chapter_info: metadata.chapter_info,
        drm,
        download_url: Secret::new(url),
    })
}

fn denial_error(raw: &RawContentLicense) -> Error {
    let message = raw
        .message
        .clone()
        .unwrap_or_else(|| format!("status {}", raw.status_code.as_deref().unwrap_or("missing")));
    let reasons = raw.license_denial_reasons.iter().map(|r| {
        format!(
            "{} {} {}",
            r.rejection_reason.as_deref().unwrap_or(""),
            r.validation_type.as_deref().unwrap_or(""),
            r.message.as_deref().unwrap_or("")
        )
        .to_ascii_lowercase()
    });
    let mut throttled = false;
    let mut drm = false;
    for text in reasons.chain(std::iter::once(message.to_ascii_lowercase())) {
        throttled |= text.contains("throttl");
        drm |= text.contains("widevine") || text.contains("drm");
    }
    if throttled {
        Error::Throttled { message }
    } else if drm {
        Error::UnsupportedDrm {
            drm_type: raw.drm_type.clone().unwrap_or_else(|| "unknown".to_owned()),
        }
    } else {
        Error::LicenseDenied { message }
    }
}

/// Decrypt an AAXC voucher (`audible.aescipher._decrypt_voucher`):
/// `sha256(device_type + serial + customer_id + asin)` gives key (first 16
/// bytes) and IV (last 16); the base64 voucher is AES-128-CBC without padding
/// and NUL-terminated JSON containing hex `key` and `iv`.
pub(crate) fn decrypt_voucher(
    voucher_b64: &str,
    creds: &Credentials,
    asin: &Asin,
) -> Result<DecryptionKey> {
    let mut hasher = Sha256::new();
    hasher.update(creds.device_type.as_bytes());
    hasher.update(creds.device_serial.as_str().as_bytes());
    hasher.update(creds.customer_id.as_bytes());
    hasher.update(asin.as_str().as_bytes());
    let digest = zeroize::Zeroizing::new(<[u8; 32]>::from(hasher.finalize()));
    let mut key = [0u8; 16];
    let mut iv = [0u8; 16];
    key.copy_from_slice(&digest[..16]);
    iv.copy_from_slice(&digest[16..]);

    let mut data = zeroize::Zeroizing::new(
        STANDARD
            .decode(voucher_b64.trim())
            .map_err(|_| Error::Crypto("voucher is not base64".into()))?,
    );
    if data.is_empty() || data.len() % BLOCK != 0 {
        return Err(Error::Crypto(
            "voucher length is not a multiple of 16".into(),
        ));
    }
    aes128_cbc_decrypt_blocks(&key, &iv, &mut data);
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let plaintext = &data[..end];

    #[derive(Deserialize)]
    struct Voucher {
        key: String,
        iv: String,
    }
    let parsed: Voucher = serde_json::from_slice(plaintext)
        .or_else(|_| parse_voucher_prefix(plaintext))
        .map_err(|_| {
            Error::Crypto(
                "voucher did not decrypt; was the device re-registered with another serial?".into(),
            )
        })?;
    match (hex16(&parsed.key), hex16(&parsed.iv)) {
        (Some(key), Some(iv)) => {
            debug!(%asin, "voucher decrypted");
            Ok(DecryptionKey::Aaxc { key, iv })
        }
        _ => Err(Error::Crypto("voucher key/iv are not 16-byte hex".into())),
    }
}

/// The Python library falls back to a regex for vouchers whose JSON is
/// truncated: `^{"key":"(.*?)","iv":"(.*?)",`.
fn parse_voucher_prefix<T: serde::de::DeserializeOwned>(plaintext: &[u8]) -> serde_json::Result<T> {
    let text = String::from_utf8_lossy(plaintext);
    let rest = text.strip_prefix("{\"key\":\"").unwrap_or_default();
    let (key, rest) = rest.split_once("\",\"iv\":\"").unwrap_or_default();
    let (iv, _) = rest.split_once('"').unwrap_or_default();
    serde_json::from_value(json!({ "key": key, "iv": iv }))
}

/// Fetch the account's AAX activation bytes with a signed request to
/// `www.audible.com/license/token` (`audible.activation_bytes`).
pub async fn activation_bytes(http: &HttpClient) -> Result<ActivationBytes> {
    let url = http.url(
        Host::Activation,
        "/license/token",
        &[
            ("player_manuf", "Audible,iPhone".to_owned()),
            ("action", "register".to_owned()),
            ("player_model", "iPhone".to_owned()),
        ],
    )?;
    let resp = http.execute(Method::GET, url, None, &[]).await?;
    extract_activation_bytes(&resp.body)
}

/// Extract activation bytes from an activation blob: the last 0x238 bytes are
/// 8 lines of 70 bytes plus `\n`; the first 4 bytes of the joined data,
/// read little-endian, are the activation bytes.
pub(crate) fn extract_activation_bytes(blob: &[u8]) -> Result<ActivationBytes> {
    const TAIL: usize = 0x238;
    let contains = |needle: &[u8]| blob.windows(needle.len()).any(|w| w == needle);
    if contains(b"BAD_LOGIN") || contains(b"Whoops") || !contains(b"group_id") || blob.len() < TAIL
    {
        return Err(Error::Auth("activation request was rejected".into()));
    }
    let tail = &blob[blob.len() - TAIL..];
    let first = &tail[..4];
    // Little-endian u32 printed as hex == bytes reversed.
    let value = u32::from_le_bytes([first[0], first[1], first[2], first[3]]);
    Ok(ActivationBytes(value.to_be_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_blob_extraction() {
        let mut blob = b"<html>group_id=1234 junk".to_vec();
        let mut lines = Vec::new();
        for i in 0..8u8 {
            let mut line = vec![i; 70];
            if i == 0 {
                line[..4].copy_from_slice(&[0xef, 0xbe, 0xad, 0x0e]);
            }
            lines.extend_from_slice(&line);
            lines.push(b'\n');
        }
        blob.extend_from_slice(&lines);
        let ab = extract_activation_bytes(&blob).unwrap();
        assert_eq!(ab.to_hex(), "0eadbeef");
        assert!(extract_activation_bytes(b"BAD_LOGIN group_id").is_err());
    }

    #[test]
    fn voucher_prefix_fallback() {
        #[derive(Deserialize)]
        struct V {
            key: String,
            iv: String,
        }
        let v: V = parse_voucher_prefix(br#"{"key":"aa","iv":"bb","rules":[trunc"#).unwrap();
        assert_eq!((v.key.as_str(), v.iv.as_str()), ("aa", "bb"));
    }
}
