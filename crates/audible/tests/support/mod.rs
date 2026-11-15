//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::time::Duration;

use audible::credentials::Secret;
use audible::http::{HttpClient, RetryPolicy};
use audible::locale::Endpoints;
use audible::signing::AdpSigner;
use wiremock::MockServer;

/// ADP token used by test clients.
pub const TOKEN: &str = "{enc:ENC}{key:KEY}{iv:IV}{name:NAME}{serial:Mg==}";

/// Read a fixture as a string.
pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// The PEM test key.
pub fn key_pem() -> String {
    fixture("test_rsa_key.pem")
}

/// A signed client pointed at the mock server, with fast retries.
pub fn client(server: &MockServer) -> HttpClient {
    let signer = AdpSigner::new(&Secret::new(TOKEN), &Secret::new(key_pem())).unwrap();
    HttpClient::new(
        reqwest::Client::new(),
        Endpoints::all(&server.uri()),
        signer,
    )
    .with_retry(RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(50),
    })
}

/// Credentials matching the fixtures (`register_response.json`).
pub fn credentials() -> audible::credentials::Credentials {
    use audible::credentials::Credentials;
    Credentials {
        marketplace: audible::Marketplace::UnitedStates,
        device_serial: audible::auth::DeviceSerial::new("Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0f")
            .unwrap(),
        device_type: "A2CZJZGLK2JJVM".into(),
        device_name: Some("Test Audible for iPhone".into()),
        customer_id: "amzn1.account.AEXAMPLECUSTOMERID".into(),
        customer_name: Some("Example Owner".into()),
        adp_token: Secret::new(TOKEN),
        device_private_key: Secret::new(key_pem()),
        access_token: Secret::new("Atna|EXAMPLE"),
        access_token_expires: chrono::Utc::now() + chrono::Duration::hours(1),
        refresh_token: Secret::new("Atnr|EXAMPLE"),
        website_cookies: Default::default(),
        store_authentication_cookie: None,
        activation_bytes: None,
    }
}
