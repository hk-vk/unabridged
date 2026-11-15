//! Building the Amazon sign-in URL and reading the redirect.
//!
//! The UI shows [`LoginSession::authorization_url`] in a web view (or the
//! system browser). The user signs in on Amazon's own pages, including CAPTCHA
//! and 2FA, so this crate never sees the password. Amazon then redirects to
//! `https://www.amazon.<tld>/ap/maplanding?...&openid.oa2.authorization_code=...`
//! (the page itself 404s; that is expected). The UI hands that URL to
//! [`LoginSession::authorization_code`] or straight to
//! [`crate::auth::register_device`].
//!
//! Parameters mirror `audible.login.build_oauth_url` (Python `audible` 0.13.1).

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use rand::RngCore;
use url::Url;

use super::device::{DeviceProfile, DeviceSerial};
use super::pkce::CodeVerifier;
use crate::error::{Error, Result};
use crate::locale::{Endpoints, Marketplace};

/// User agent the official app's web view sends (from `audible.login`).
/// Web views presenting the login page should use it so Amazon serves the
/// mobile app flow.
pub const LOGIN_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) \
     AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

const CODE_PARAM: &str = "openid.oa2.authorization_code";

/// A cookie the web view should set on the Amazon domain before loading the
/// sign-in URL. These mirror `audible.login.build_init_cookies` and reduce
/// CAPTCHA prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCookie {
    /// Cookie name.
    pub name: &'static str,
    /// Cookie value.
    pub value: String,
    /// Cookie domain, e.g. `.amazon.com`.
    pub domain: String,
}

/// One in-progress sign-in attempt. Keep it alive until the redirect arrives:
/// the code verifier and serial inside are needed to register the device.
#[derive(Debug, Clone)]
pub struct LoginSession {
    marketplace: Marketplace,
    endpoints: Endpoints,
    serial: DeviceSerial,
    verifier: CodeVerifier,
    profile: DeviceProfile,
}

impl LoginSession {
    /// Start a sign-in for `marketplace` with a fresh serial and verifier.
    pub fn new(marketplace: Marketplace) -> Self {
        Self::with_parts(
            marketplace,
            marketplace.endpoints(),
            DeviceSerial::generate(),
            CodeVerifier::generate(),
        )
    }

    /// Start a sign-in with explicit parts (tests, custom endpoints).
    pub fn with_parts(
        marketplace: Marketplace,
        endpoints: Endpoints,
        serial: DeviceSerial,
        verifier: CodeVerifier,
    ) -> Self {
        Self {
            marketplace,
            endpoints,
            serial,
            verifier,
            profile: DeviceProfile::IPHONE,
        }
    }

    /// Marketplace being signed in to.
    pub fn marketplace(&self) -> Marketplace {
        self.marketplace
    }

    /// Endpoints used for registration.
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// Device serial that will be registered.
    pub fn serial(&self) -> &DeviceSerial {
        &self.serial
    }

    /// PKCE verifier.
    pub fn verifier(&self) -> &CodeVerifier {
        &self.verifier
    }

    /// Device profile.
    pub fn profile(&self) -> &DeviceProfile {
        &self.profile
    }

    /// The URL to open in a web view.
    ///
    /// Fails only when a custom [`Endpoints::amazon_www`] is not a valid URL.
    pub fn authorization_url(&self) -> Result<Url> {
        let m = self.marketplace;
        let www = &self.endpoints.amazon_www;
        let return_to = format!("{www}/ap/maplanding");
        let client_id = format!("device:{}", self.profile.client_id(&self.serial));
        let challenge = self.verifier.challenge();
        let assoc_handle = format!("amzn_audible_ios_{}", m.country_code());
        let params: [(&str, &str); 18] = [
            ("openid.oa2.response_type", "code"),
            ("openid.oa2.code_challenge_method", "S256"),
            ("openid.oa2.code_challenge", &challenge),
            ("openid.return_to", &return_to),
            ("openid.assoc_handle", &assoc_handle),
            (
                "openid.identity",
                "http://specs.openid.net/auth/2.0/identifier_select",
            ),
            ("pageId", "amzn_audible_ios"),
            ("accountStatusPolicy", "P1"),
            (
                "openid.claimed_id",
                "http://specs.openid.net/auth/2.0/identifier_select",
            ),
            ("openid.mode", "checkid_setup"),
            ("openid.ns.oa2", "http://www.amazon.com/ap/ext/oauth/2"),
            ("openid.oa2.client_id", &client_id),
            (
                "openid.ns.pape",
                "http://specs.openid.net/extensions/pape/1.0",
            ),
            ("marketPlaceId", m.marketplace_id()),
            ("openid.oa2.scope", "device_auth_access"),
            ("forceMobileLayout", "true"),
            ("openid.ns", "http://specs.openid.net/auth/2.0"),
            ("openid.pape.max_auth_age", "0"),
        ];
        let mut url = Url::parse(&format!("{www}/ap/signin"))
            .map_err(|e| Error::InvalidArgument(format!("bad sign-in endpoint: {e}")))?;
        url.query_pairs_mut().extend_pairs(params);
        Ok(url)
    }

    /// Cookies to set before loading [`Self::authorization_url`].
    pub fn initial_cookies(&self) -> Vec<LoginCookie> {
        let domain = format!(".amazon.{}", self.marketplace.domain());
        let mut frc = [0u8; 313];
        rand::rngs::OsRng.fill_bytes(&mut frc);
        let map_md = serde_json::json!({
            "device_user_dictionary": [],
            "device_registration_data": {"software_version": self.profile.software_version},
            "app_identifier": {
                "app_version": self.profile.app_version,
                "bundle_id": self.profile.bundle_id,
            },
        });
        vec![
            LoginCookie {
                name: "frc",
                value: STANDARD_NO_PAD.encode(frc),
                domain: domain.clone(),
            },
            LoginCookie {
                name: "map-md",
                value: STANDARD_NO_PAD.encode(map_md.to_string()),
                domain: domain.clone(),
            },
            LoginCookie {
                name: "amzn-app-id",
                value: "MAPiOSLib/6.0/ToHideRetailLink".to_owned(),
                domain,
            },
        ]
    }

    /// Whether a URL the web view navigated to is the sign-in redirect. The
    /// UI can watch navigations and stop as soon as this returns `true`.
    pub fn is_redirect(url: &str) -> bool {
        Url::parse(url).is_ok_and(|u| {
            u.path().ends_with("/ap/maplanding") && u.query_pairs().any(|(k, _)| k == CODE_PARAM)
        })
    }

    /// Extract the authorization code from the redirect URL.
    pub fn authorization_code(redirect_url: &str) -> Result<String> {
        let url = Url::parse(redirect_url.trim())
            .map_err(|e| Error::Auth(format!("redirect is not a URL: {e}")))?;
        url.query_pairs()
            .find(|(k, _)| k == CODE_PARAM)
            .map(|(_, v)| v.into_owned())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                Error::Auth("redirect URL has no openid.oa2.authorization_code".to_owned())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn session() -> LoginSession {
        LoginSession::with_parts(
            Marketplace::UnitedKingdom,
            Marketplace::UnitedKingdom.endpoints(),
            DeviceSerial::new("A".repeat(40)).unwrap(),
            CodeVerifier::from_string("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        )
    }

    #[test]
    fn authorization_url_matches_reference_parameters() {
        let url = session().authorization_url().unwrap();
        assert_eq!(url.host_str(), Some("www.amazon.co.uk"));
        assert_eq!(url.path(), "/ap/signin");
        let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            q["openid.oa2.code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(q["openid.oa2.code_challenge_method"], "S256");
        assert_eq!(q["openid.assoc_handle"], "amzn_audible_ios_uk");
        assert_eq!(q["marketPlaceId"], "A2I9A3Q2GNFNGQ");
        assert_eq!(
            q["openid.return_to"],
            "https://www.amazon.co.uk/ap/maplanding"
        );
        assert!(q["openid.oa2.client_id"].starts_with("device:4141"));
        assert_eq!(q["openid.oa2.scope"], "device_auth_access");
        assert_eq!(q.len(), 18);
    }

    #[test]
    fn extracts_code_from_redirect() {
        let redirect = "https://www.amazon.com/ap/maplanding?openid.assoc_handle=amzn_audible_ios_us\
                        &openid.oa2.authorization_code=ANxyzCODE123&openid.mode=id_res";
        assert!(LoginSession::is_redirect(redirect));
        assert_eq!(
            LoginSession::authorization_code(redirect).unwrap(),
            "ANxyzCODE123"
        );
        assert!(!LoginSession::is_redirect(
            "https://www.amazon.com/ap/signin?x=1"
        ));
        assert!(LoginSession::authorization_code("https://www.amazon.com/ap/maplanding").is_err());
        assert!(LoginSession::authorization_code("not a url").is_err());
    }

    #[test]
    fn initial_cookies_decode_to_expected_metadata() {
        let cookies = session().initial_cookies();
        let map_md = cookies.iter().find(|c| c.name == "map-md").unwrap();
        let json = STANDARD_NO_PAD.decode(&map_md.value).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(v["app_identifier"]["bundle_id"], "com.audible.iphone");
        assert!(cookies.iter().all(|c| c.domain == ".amazon.co.uk"));
        let frc = cookies.iter().find(|c| c.name == "frc").unwrap();
        assert_eq!(STANDARD_NO_PAD.decode(&frc.value).unwrap().len(), 313);
    }
}
