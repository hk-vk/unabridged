//! Transport tests: signing on the wire, retries, error mapping.

mod support;

use std::time::Duration;

use audible::Error;
use base64::Engine;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::signature::Verifier;
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use support::{TOKEN, client, key_pem};

/// Recompute the signed string from what actually arrived and verify it
/// with the public key.
fn verify_signature(req: &Request) {
    let sig_header = req
        .headers
        .get("x-adp-signature")
        .unwrap()
        .to_str()
        .unwrap();
    let (sig_b64, date) = sig_header.split_once(':').unwrap();
    let target = match req.url.query() {
        Some(q) => format!("{}?{q}", req.url.path()),
        None => req.url.path().to_owned(),
    };
    let mut data = format!("{}\n{target}\n{date}\n", req.method).into_bytes();
    data.extend_from_slice(&req.body);
    data.extend_from_slice(format!("\n{TOKEN}").as_bytes());

    let private = rsa::RsaPrivateKey::from_pkcs1_pem(&key_pem()).unwrap();
    let verifying = rsa::pkcs1v15::VerifyingKey::<sha2::Sha256>::new(private.to_public_key());
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(sig_b64)
        .unwrap();
    let signature = rsa::pkcs1v15::Signature::try_from(sig_bytes.as_slice()).unwrap();
    verifying
        .verify(&data, &signature)
        .expect("signature verifies against wire request");
    assert!(date.ends_with("+00:00Z"));
}

#[tokio::test]
async fn signs_get_and_post_exactly_as_sent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.0/library"))
        .and(header("x-adp-token", TOKEN))
        .and(header("x-adp-alg", "SHA256withRSA:1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"items":[]}"#))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/1.0/content/B0TEST/licenserequest"))
        .and(header_exists("x-adp-signature"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&server)
        .await;

    let c = client(&server);
    let _: serde_json::Value = c
        .get_json(
            "/1.0/library",
            &[
                ("response_groups", "media, product_attrs".into()),
                ("page", "1".into()),
            ],
        )
        .await
        .unwrap();
    let _: serde_json::Value = c
        .post_json(
            "/1.0/content/B0TEST/licenserequest",
            &serde_json::json!({"quality": "High"}),
            &[],
        )
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    for req in &requests {
        verify_signature(req);
    }
    assert_eq!(
        requests[0].url.query(),
        Some("response_groups=media%2C+product_attrs&page=1")
    );
}

#[tokio::test]
async fn retries_503_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/library"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(path("/1.0/library"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
        .mount(&server)
        .await;
    let v: serde_json::Value = client(&server).get_json("/1.0/library", &[]).await.unwrap();
    assert_eq!(v["ok"], true);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    // Each attempt is signed afresh.
    requests.iter().for_each(verify_signature);
}

#[tokio::test]
async fn rate_limit_exhausts_into_typed_error() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/library"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .expect(3)
        .mount(&server)
        .await;
    let err = client(&server)
        .get_json::<serde_json::Value>("/1.0/library", &[])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::RateLimited { retry_after: Some(d) } if d == Duration::ZERO),
        "{err:?}"
    );
    assert!(err.is_transient());
}

#[tokio::test]
async fn post_is_not_retried_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string(r#"{"message":"boom"}"#))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server)
        .post_json::<_, serde_json::Value>("/1.0/x", &serde_json::json!({}), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Api { status: 500, message } if message == "boom"),
        "{err:?}"
    );
}

#[tokio::test]
async fn maps_401_and_404() {
    let server = MockServer::start().await;
    Mock::given(path("/1.0/a"))
        .respond_with(ResponseTemplate::new(401).set_body_string(r#"{"message":"Invalid token"}"#))
        .mount(&server)
        .await;
    Mock::given(path("/1.0/b"))
        .respond_with(ResponseTemplate::new(404).set_body_string(r#"{"error":"not here"}"#))
        .mount(&server)
        .await;
    let c = client(&server);
    let a = c
        .get_json::<serde_json::Value>("/1.0/a", &[])
        .await
        .unwrap_err();
    assert!(a.requires_sign_in(), "{a:?}");
    let b = c
        .get_json::<serde_json::Value>("/1.0/b", &[])
        .await
        .unwrap_err();
    assert!(matches!(b, Error::NotFound { .. }));
    // 4xx is not retried.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
