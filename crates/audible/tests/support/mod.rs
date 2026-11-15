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
