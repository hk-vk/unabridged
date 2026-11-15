//! ADP request signing, as the official apps do.
//!
//! Every API request carries three headers (from `audible.auth.sign_request`):
//!
//! * `x-adp-token`: the ADP token from registration.
//! * `x-adp-alg`: `SHA256withRSA:1.0`.
//! * `x-adp-signature`: `base64(RSASSA-PKCS1-v1_5-SHA256(data)) + ":" + date`
//!
//! where `data = "{METHOD}\n{path?query}\n{date}\n{body}\n{adp_token}"` and
//! `date` is the current UTC time in the exact form Python's
//! `datetime.now(UTC).isoformat("T") + "Z"` produces
//! (`2026-11-15T12:34:56.789012+00:00Z`).

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use sha2::Sha256;

use crate::credentials::Secret;
use crate::error::{Error, Result};

/// Value of the `x-adp-alg` header.
pub const ADP_ALGORITHM: &str = "SHA256withRSA:1.0";

/// The three signing headers for one request.
#[derive(Clone, PartialEq, Eq)]
pub struct SignedHeaders {
    /// `x-adp-token`.
    pub adp_token: String,
    /// `x-adp-alg`.
    pub algorithm: &'static str,
    /// `x-adp-signature`.
    pub signature: String,
}

impl SignedHeaders {
    /// Header name/value pairs, ready to add to a request.
    pub fn pairs(&self) -> [(&'static str, &str); 3] {
        [
            ("x-adp-token", self.adp_token.as_str()),
            ("x-adp-alg", self.algorithm),
            ("x-adp-signature", self.signature.as_str()),
        ]
    }
}

impl fmt::Debug for SignedHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignedHeaders(<redacted>)")
    }
}

/// Signs requests with a registered device's key. Parsing the PEM key is
/// done once; signing is then cheap and thread-safe.
#[derive(Clone)]
pub struct AdpSigner {
    adp_token: Secret,
    key: SigningKey<Sha256>,
}

impl fmt::Debug for AdpSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdpSigner(<redacted>)")
    }
}

impl AdpSigner {
    /// Build a signer from the registration's ADP token and PEM private key
    /// (PKCS#1 `RSA PRIVATE KEY`, as Amazon returns it, or PKCS#8).
    pub fn new(adp_token: &Secret, private_key_pem: &Secret) -> Result<Self> {
        let pem = private_key_pem.expose().trim();
        let key = RsaPrivateKey::from_pkcs1_pem(pem)
            .or_else(|_| RsaPrivateKey::from_pkcs8_pem(pem))
            .map_err(|_| Error::Crypto("device private key is not a valid RSA PEM key".into()))?;
        Ok(Self {
            adp_token: adp_token.clone(),
            key: SigningKey::<Sha256>::new(key),
        })
    }

    /// Sign a request now.
    ///
    /// `path_and_query` is the raw request target, e.g.
    /// `/1.0/library?num_results=50&page=1`; it must byte-for-byte match what
    /// goes on the wire.
    pub fn sign(&self, method: &str, path_and_query: &str, body: &[u8]) -> SignedHeaders {
        self.sign_at(method, path_and_query, body, Utc::now())
    }

    /// Sign a request with an explicit timestamp (deterministic tests).
    pub fn sign_at(
        &self,
        method: &str,
        path_and_query: &str,
        body: &[u8],
        at: DateTime<Utc>,
    ) -> SignedHeaders {
        let date = format_date(at);
        let token = self.adp_token.expose();
        let mut data =
            Vec::with_capacity(method.len() + path_and_query.len() + body.len() + token.len() + 48);
        data.extend_from_slice(method.as_bytes());
        data.push(b'\n');
        data.extend_from_slice(path_and_query.as_bytes());
        data.push(b'\n');
        data.extend_from_slice(date.as_bytes());
        data.push(b'\n');
        data.extend_from_slice(body);
        data.push(b'\n');
        data.extend_from_slice(token.as_bytes());

        let signature = self.key.sign(&data);
        SignedHeaders {
            adp_token: token.to_owned(),
            algorithm: ADP_ALGORITHM,
            signature: format!("{}:{date}", STANDARD.encode(signature.to_bytes())),
        }
    }
}

/// Python `datetime.isoformat("T") + "Z"` for an aware UTC datetime.
fn format_date(at: DateTime<Utc>) -> String {
    format!("{}+00:00Z", at.format("%Y-%m-%dT%H:%M:%S%.6f"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const TOKEN: &str = "{enc:ENC}{key:KEY}{iv:IV}{name:NAME}{serial:Mg==}";

    fn key(name: &str) -> Secret {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        Secret::new(std::fs::read_to_string(path).unwrap())
    }

    fn at(micros: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 15, 12, 34, 56)
            .unwrap()
            .checked_add_signed(chrono::Duration::microseconds(i64::from(micros)))
            .unwrap()
    }

    #[test]
    fn date_matches_python_isoformat() {
        assert_eq!(
            format_date(at(789_012)),
            "2026-11-15T12:34:56.789012+00:00Z"
        );
        assert_eq!(format_date(at(1)), "2026-11-15T12:34:56.000001+00:00Z");
    }

    /// Expected values from
    /// `openssl dgst -sha256 -sign tests/fixtures/test_rsa_key.pem data | base64`.
    #[test]
    fn known_answer_get_without_body() {
        let signer = AdpSigner::new(&Secret::new(TOKEN), &key("test_rsa_key.pem")).unwrap();
        let h = signer.sign_at(
            "GET",
            "/1.0/library?num_results=10&page=1",
            b"",
            at(789_012),
        );
        assert_eq!(
            h.signature,
            "klGHpMDNTQ2PKWsMHnftrILq66QfN3YDDrOczZA0+KpMIRglNsMMpD/JtNv90BnEVvNR1pB0TFaMXYX/A/cc\
             wHfN4jZ2tiCRV/Ql5NSClPyWte3PQl9spn21hLuiHvvUKCYfahZyttp4dwucp3czdkUPoNY3ua/1yN9k0SAfx\
             HY=:2026-11-15T12:34:56.789012+00:00Z"
        );
        assert_eq!(h.adp_token, TOKEN);
        assert_eq!(h.algorithm, "SHA256withRSA:1.0");
    }

    #[test]
    fn known_answer_post_with_body_and_pkcs8_key() {
        let signer = AdpSigner::new(&Secret::new(TOKEN), &key("test_rsa_key_pkcs8.pem")).unwrap();
        let h = signer.sign_at(
            "POST",
            "/1.0/content/B002V5D7RU/licenserequest",
            br#"{"quality":"High"}"#,
            at(1),
        );
        assert_eq!(
            h.signature,
            "l0auS6/PTWzNgCbp2Oh+S7U/sLgEQLWb0mOK8vRcWS4L+8VsoN2mQ008utI24IQG2hw9zVPbJJeNnWpePKrT\
             x702lYykA8kuXNVJAqZlISotCOvgKTaB5t44G2jaJ+GTAKE12D5Ln71WGoYVUuxeKQBXScrdr7EdBbFpocE8H\
             NA=:2026-11-15T12:34:56.000001+00:00Z"
        );
    }

    #[test]
    fn rejects_garbage_keys_without_echoing_them() {
        let err = AdpSigner::new(&Secret::new(TOKEN), &Secret::new("-----BEGIN nope")).unwrap_err();
        assert!(!err.to_string().contains("nope"));
    }

    #[test]
    fn debug_is_redacted() {
        let signer = AdpSigner::new(&Secret::new(TOKEN), &key("test_rsa_key.pem")).unwrap();
        assert!(!format!("{signer:?}").contains("ENC"));
        assert!(!format!("{:?}", signer.sign("GET", "/", b"")).contains("ENC"));
    }
}
