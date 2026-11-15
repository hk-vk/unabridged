//! PKCE (RFC 7636) code verifier and S256 challenge.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// A PKCE code verifier: 32 random bytes, base64url without padding, as in
/// `audible.login.create_code_verifier`.
#[derive(Clone)]
pub struct CodeVerifier(Zeroizing<String>);

impl CodeVerifier {
    /// Generate a new verifier from the OS RNG.
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(bytes.as_mut());
        Self(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref())))
    }

    /// Wrap an existing verifier (tests, resumed sessions).
    pub fn from_string(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The verifier string sent at registration.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The S256 challenge sent in the authorization URL.
    pub fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.0.as_bytes()))
    }
}

impl fmt::Debug for CodeVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodeVerifier(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_appendix_b_vector() {
        let v = CodeVerifier::from_string("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(v.challenge(), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generated_verifier_is_43_url_safe_chars() {
        let v = CodeVerifier::generate();
        assert_eq!(v.as_str().len(), 43);
        assert!(
            v.as_str()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert!(!format!("{v:?}").contains(v.as_str()));
    }
}
