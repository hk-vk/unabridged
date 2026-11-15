//! Registration, refresh and credential storage against a mock server.

use audible::auth::{
    CodeVerifier, DeviceSerial, LoginSession, refresh_access_token, register_device,
};
use audible::credentials::{CredentialStore, MemoryStore, Secret};
use audible::{Endpoints, Error, Marketplace};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SERIAL: &str = "Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0f";
const REDIRECT: &str = "https://www.amazon.com/ap/maplanding?openid.oa2.authorization_code=ANcode123&openid.mode=id_res";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn session(server: &MockServer) -> LoginSession {
    LoginSession::with_parts(
        Marketplace::UnitedStates,
        Endpoints::all(&server.uri()),
        DeviceSerial::new(SERIAL).unwrap(),
        CodeVerifier::from_string("test-verifier"),
    )
}

#[tokio::test]
async fn register_sends_reference_body_and_parses_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/register"))
        .and(body_partial_json(serde_json::json!({
            "registration_data": {"device_serial": SERIAL, "device_type": "A2CZJZGLK2JJVM", "app_name": "Audible"},
            "auth_data": {"authorization_code": "ANcode123", "code_verifier": "test-verifier", "code_algorithm": "SHA-256", "client_domain": "DeviceLegacy"},
            "requested_extensions": ["device_info", "customer_info"],
            "cookies": {"domain": ".amazon.com"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("register_response.json")))
        .expect(1)
        .mount(&server)
        .await;

    let creds = register_device(&reqwest::Client::new(), &session(&server), REDIRECT)
        .await
        .unwrap();
    assert_eq!(creds.customer_id, "amzn1.account.AEXAMPLECUSTOMERID");
    assert_eq!(creds.device_serial.as_str(), SERIAL);
    assert_eq!(creds.refresh_token.expose(), "Atnr|EXAMPLEREFRESHTOKEN");
    assert_eq!(
        creds.website_cookies["session-id"].expose(),
        "example-session-id"
    );
    assert!(!creds.access_token_expires_within(chrono::Duration::minutes(30)));

    let debug = format!("{creds:?}");
    assert!(
        !debug.contains("Atnr|"),
        "Debug must redact tokens: {debug}"
    );
    assert!(!debug.contains("EXAMPLEKEY"));
}

#[tokio::test]
async fn register_error_is_typed_and_redacted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/register"))
        .respond_with(ResponseTemplate::new(400).set_body_string(fixture("register_error.json")))
        .mount(&server)
        .await;
    let err = register_device(&reqwest::Client::new(), &session(&server), REDIRECT)
        .await
        .unwrap_err();
    match err {
        Error::Auth(msg) => assert!(msg.contains("InvalidValue"), "{msg}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn register_rejects_redirect_without_code() {
    let server = MockServer::start().await;
    let err = register_device(
        &reqwest::Client::new(),
        &session(&server),
        "https://www.amazon.com/ap/maplanding?openid.mode=cancel",
    )
    .await
    .unwrap_err();
    assert!(err.requires_sign_in());
}

#[tokio::test]
async fn refresh_posts_form_and_parses_expiry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/token"))
        .and(body_string_contains("source_token=Atnr%7Crefresh"))
        .and(body_string_contains("requested_token_type=access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("token_refresh.json")))
        .mount(&server)
        .await;
    let token = refresh_access_token(
        &reqwest::Client::new(),
        &Endpoints::all(&server.uri()),
        &Secret::new("Atnr|refresh"),
    )
    .await
    .unwrap();
    assert_eq!(token.token.expose(), "Atna|REFRESHEDACCESSTOKEN");
    assert!(token.expires > chrono::Utc::now() + chrono::Duration::minutes(59));
}

#[tokio::test]
async fn refresh_rejection_requires_sign_in() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_string(
            r#"{"error":"invalid_grant","error_description":"The request has an invalid grant parameter : refresh_token"}"#,
        ))
        .mount(&server)
        .await;
    let err = refresh_access_token(
        &reqwest::Client::new(),
        &Endpoints::all(&server.uri()),
        &Secret::new("Atnr|stale"),
    )
    .await
    .unwrap_err();
    assert!(err.requires_sign_in(), "{err:?}");
}

#[tokio::test]
async fn memory_store_round_trips_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/register"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("register_response.json")))
        .mount(&server)
        .await;
    let creds = register_device(&reqwest::Client::new(), &session(&server), REDIRECT)
        .await
        .unwrap();

    let store = MemoryStore::new();
    assert!(store.load("default").unwrap().is_none());
    store.save("default", &creds).unwrap();
    assert_eq!(store.load("default").unwrap().as_ref(), Some(&creds));
    store.delete("default").unwrap();
    store.delete("default").unwrap();
    assert!(store.load("default").unwrap().is_none());
}

/// Touches the real login Keychain; run manually with `--ignored`.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "writes to the real macOS Keychain"]
async fn keychain_store_round_trips() {
    use audible::credentials::KeychainStore;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/register"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("register_response.json")))
        .mount(&server)
        .await;
    let creds = register_device(&reqwest::Client::new(), &session(&server), REDIRECT)
        .await
        .unwrap();
    let store = KeychainStore::with_service("app.betteraudible.credentials.test");
    store.save("test-profile", &creds).unwrap();
    assert_eq!(store.load("test-profile").unwrap(), Some(creds));
    store.delete("test-profile").unwrap();
    assert!(store.load("test-profile").unwrap().is_none());
}
