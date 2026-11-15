//! Content metadata, licenses (voucher, DRM detection) and activation bytes.

mod support;

use audible::api::content::{self, ContentDrm};
use audible::drm::DecryptionKey;
use audible::models::Quality;
use audible::{Asin, Error};
use support::{client, credentials, fixture};
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn hobbit() -> Asin {
    Asin::new("B002V5D7RU").unwrap()
}

async fn license_server(fixture_name: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex_any())
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture(fixture_name)))
        .mount(&server)
        .await;
    server
}

fn path_regex_any() -> wiremock::matchers::PathRegexMatcher {
    wiremock::matchers::path_regex(r"^/1\.0/content/[A-Z0-9]+/licenserequest$")
}

#[tokio::test]
async fn metadata_and_chapters() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/content/B002V5D7RU/metadata"))
        .and(query_param("chapter_titles_type", "Tree"))
        .and(query_param("drm_type", "Adrm"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("content_metadata.json")))
        .mount(&server)
        .await;
    let c = client(&server);
    let meta = content::metadata(&c, &hobbit(), Quality::High)
        .await
        .unwrap();
    assert_eq!(
        meta.content_reference.unwrap().acr,
        "CR!7XQ4B3JH9V0KQ8WZ0Y0RNX6MCB1E"
    );
    let chapters = content::chapters(&c, &hobbit()).await.unwrap();
    assert_eq!(chapters.flatten().len(), 5);
}

/// The voucher fixture was encrypted with `openssl enc -aes-128-cbc -nopad`
/// using key/iv from Python `hashlib.sha256(device_type + serial +
/// customer_id + asin)`, independently of this crate.
#[tokio::test]
async fn aaxc_license_decrypts_voucher() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/1.0/content/B002V5D7RU/licenserequest"))
        .and(body_partial_json(serde_json::json!({
            "supported_drm_types": ["Mpeg", "Adrm"],
            "quality": "High",
            "consumption_type": "Download"
        })))
        .and(header("X-Device-Type-Id", "A2CZJZGLK2JJVM"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("license_aaxc.json")))
        .mount(&server)
        .await;
    let lic = content::license(&client(&server), &credentials(), &hobbit(), Quality::High)
        .await
        .unwrap();
    let ContentDrm::Aaxc(DecryptionKey::Aaxc { key, iv }) = &lic.drm else {
        panic!("expected AAXC key, got {:?}", lic.drm);
    };
    assert_eq!(hex::encode(key), "0123456789abcdef0123456789abcdef");
    assert_eq!(hex::encode(iv), "fedcba9876543210fedcba9876543210");
    assert_eq!(lic.acr(), Some("CR!7XQ4B3JH9V0KQ8WZ0Y0RNX6MCB1E"));
    assert!(lic.download_url.expose().contains(".aaxc"));
    assert!(
        !format!("{lic:?}").contains("Signature="),
        "URL is redacted in Debug"
    );
    assert!(
        !format!("{lic:?}").contains("0123456789abcdef"),
        "key is redacted in Debug"
    );
}

#[tokio::test]
async fn wrong_device_cannot_decrypt_voucher() {
    let server = license_server("license_aaxc.json").await;
    let mut creds = credentials();
    creds.customer_id = "amzn1.account.SOMEONEELSE".into();
    let err = content::license(&client(&server), &creds, &hobbit(), Quality::High)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Crypto(_)), "{err:?}");
}

#[tokio::test]
async fn throttled_license_is_typed() {
    let server = license_server("license_throttled.json").await;
    let err = content::license(&client(&server), &credentials(), &hobbit(), Quality::High)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Throttled { .. }), "{err:?}");
    assert!(err.is_transient());
}

#[tokio::test]
async fn widevine_only_title_is_unsupported() {
    let server = license_server("license_widevine.json").await;
    let err = content::license(
        &client(&server),
        &credentials(),
        &Asin::new("B0D7WIDEVN").unwrap(),
        Quality::High,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, Error::UnsupportedDrm { drm_type } if drm_type == "Widevine"),
        "{err:?}"
    );
}

#[tokio::test]
async fn denial_mentioning_drm_is_unsupported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex_any())
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"content_license":{"asin":"B0X","status_code":"Denied","message":"Requested drm type is not supported for this content: Widevine required"}}"#,
        ))
        .mount(&server)
        .await;
    let err = content::license(
        &client(&server),
        &credentials(),
        &Asin::new("B0X").unwrap(),
        Quality::High,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::UnsupportedDrm { .. }), "{err:?}");
}

#[tokio::test]
async fn plain_denial_is_license_denied() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex_any())
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"content_license":{"asin":"B0X","status_code":"Denied","message":"Not owned","license_denial_reasons":[{"rejectionReason":"NotOwned","validationType":"Ownership"}]}}"#,
        ))
        .mount(&server)
        .await;
    let err = content::license(
        &client(&server),
        &credentials(),
        &Asin::new("B0X").unwrap(),
        Quality::High,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, Error::LicenseDenied { message } if message == "Not owned"),
        "{err:?}"
    );
}

#[tokio::test]
async fn mpeg_license_needs_no_key() {
    let server = license_server("license_mpeg.json").await;
    let lic = content::license(
        &client(&server),
        &credentials(),
        &Asin::new("B0PODCAST1").unwrap(),
        Quality::Normal,
    )
    .await
    .unwrap();
    assert_eq!(lic.drm, ContentDrm::None);
}

#[tokio::test]
async fn activation_bytes_are_signed_and_extracted() {
    let server = MockServer::start().await;
    let mut blob = b"group_id=12345\n".to_vec();
    for i in 0..8u8 {
        let mut line = vec![0x30 + i; 70];
        if i == 0 {
            line[..4].copy_from_slice(&[0x78, 0x56, 0x34, 0x12]);
        }
        blob.extend_from_slice(&line);
        blob.push(b'\n');
    }
    Mock::given(method("GET"))
        .and(path("/license/token"))
        .and(query_param("action", "register"))
        .and(query_param("player_manuf", "Audible,iPhone"))
        .and(wiremock::matchers::header_exists("x-adp-signature"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(blob))
        .mount(&server)
        .await;
    let ab = content::activation_bytes(&client(&server)).await.unwrap();
    assert_eq!(ab.to_hex(), "12345678");
}
