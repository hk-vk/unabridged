//! Device registration, deregistration and access-token refresh against
//! `api.amazon.<tld>/auth/*`. Request bodies mirror `audible.register` and
//! `audible.auth.refresh_access_token` (Python `audible` 0.13.1).

use std::collections::BTreeMap;

use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info};

use super::device::DeviceSerial;
use super::login::LoginSession;
use crate::credentials::{Credentials, Secret};
use crate::error::{Error, Result};
use crate::locale::Endpoints;

/// A freshly refreshed access token.
#[derive(Debug, Clone)]
pub struct AccessToken {
    /// The token (`Atna|…`).
    pub token: Secret,
    /// Absolute expiry.
    pub expires: chrono::DateTime<Utc>,
}

/// Exchange the sign-in redirect for a registered device.
///
/// `redirect_url` is the `/ap/maplanding` URL the web view landed on.
pub async fn register_device(
    http: &reqwest::Client,
    session: &LoginSession,
    redirect_url: &str,
) -> Result<Credentials> {
    let code = LoginSession::authorization_code(redirect_url)?;
    let profile = session.profile();
    let serial = session.serial();
    let body = json!({
        "requested_token_type": ["bearer", "mac_dms", "website_cookies", "store_authentication_cookie"],
        "cookies": {"website_cookies": [], "domain": format!(".amazon.{}", session.marketplace().domain())},
        "registration_data": {
            "domain": "Device",
            "app_version": profile.app_version,
            "device_serial": serial.as_str(),
            "device_type": profile.device_type,
            "device_name": profile.device_name,
            "os_version": profile.os_version,
            "software_version": profile.software_version,
            "device_model": profile.device_model,
            "app_name": "Audible",
        },
        "auth_data": {
            "client_id": profile.client_id(serial),
            "authorization_code": code,
            "code_verifier": session.verifier().as_str(),
            "code_algorithm": "SHA-256",
            "client_domain": "DeviceLegacy",
        },
        "requested_extensions": ["device_info", "customer_info"],
    });

    let url = format!("{}/auth/register", session.endpoints().amazon_api);
    debug!(marketplace = ?session.marketplace(), "registering device");
    let resp = http.post(&url).json(&body).send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(Error::Auth(format!(
            "device registration failed ({}): {}",
            status.as_u16(),
            auth_error_message(&bytes)
        )));
    }
    let parsed: RegisterResponse =
        serde_json::from_slice(&bytes).map_err(|e| Error::decode("auth/register", e))?;
    let creds = parsed.into_credentials(session)?;
    info!(device = ?creds.device_name, "device registered");
    Ok(creds)
}

/// Refresh the bearer access token with the refresh token.
pub async fn refresh_access_token(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    refresh_token: &Secret,
) -> Result<AccessToken> {
    let form = [
        ("app_name", "Audible"),
        ("app_version", super::DeviceProfile::IPHONE.app_version),
        ("source_token", refresh_token.expose()),
        ("requested_token_type", "access_token"),
        ("source_token_type", "refresh_token"),
    ];
    let url = format!("{}/auth/token", endpoints.amazon_api);
    let resp = http.post(&url).form(&form).send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        let message = auth_error_message(&bytes);
        return Err(if status.as_u16() == 400 || status.as_u16() == 401 {
            Error::Unauthorized {
                status: status.as_u16(),
                message: format!("token refresh rejected: {message}"),
            }
        } else {
            Error::Api {
                status: status.as_u16(),
                message,
            }
        });
    }
    let parsed: TokenResponse =
        serde_json::from_slice(&bytes).map_err(|e| Error::decode("auth/token", e))?;
    debug!(expires_in = parsed.expires_in.0, "access token refreshed");
    Ok(AccessToken {
        token: Secret::new(parsed.access_token),
        expires: Utc::now() + Duration::seconds(parsed.expires_in.0),
    })
}

/// Deregister this device (or every device with `all = true`). After this
/// the credentials are useless and should be deleted.
pub async fn deregister_device(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    access_token: &Secret,
    all: bool,
) -> Result<()> {
    let url = format!("{}/auth/deregister", endpoints.amazon_api);
    let resp = http
        .post(&url)
        .header("x-amz-access-token", access_token.expose())
        .json(&json!({ "deregister_all_existing_accounts": all }))
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        info!("device deregistered");
        return Ok(());
    }
    let bytes = resp.bytes().await?;
    Err(Error::Auth(format!(
        "deregistration failed ({}): {}",
        status.as_u16(),
        auth_error_message(&bytes)
    )))
}

/// Pull `response.error.message` (or a generic message) out of an
/// `/auth/*` error body without echoing tokens.
fn auth_error_message(body: &[u8]) -> String {
    #[derive(Deserialize)]
    struct Envelope {
        response: Option<Inner>,
        error: Option<String>,
        error_description: Option<String>,
    }
    #[derive(Deserialize)]
    struct Inner {
        error: Option<ErrorBody>,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        code: Option<String>,
        message: Option<String>,
    }
    match serde_json::from_slice::<Envelope>(body) {
        Ok(env) => {
            if let Some(err) = env.response.and_then(|r| r.error) {
                return format!(
                    "{}: {}",
                    err.code.unwrap_or_default(),
                    err.message.unwrap_or_default()
                );
            }
            env.error_description
                .or(env.error)
                .unwrap_or_else(|| "unknown error".to_owned())
        }
        Err(_) => "unreadable error body".to_owned(),
    }
}

/// Accepts `3600` or `"3600"`.
#[derive(Debug, Clone, Copy)]
struct Seconds(i64);

impl<'de> Deserialize<'de> for Seconds {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Int(i64),
            Str(String),
        }
        match Raw::deserialize(d)? {
            Raw::Int(i) => Ok(Seconds(i)),
            Raw::Str(s) => s
                .trim()
                .parse()
                .map(Seconds)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Seconds,
}

#[derive(Deserialize)]
struct RegisterResponse {
    response: RegisterInner,
}

#[derive(Deserialize)]
struct RegisterInner {
    success: RegisterSuccess,
}

#[derive(Deserialize)]
struct RegisterSuccess {
    tokens: Tokens,
    extensions: Extensions,
}

#[derive(Deserialize)]
struct Tokens {
    mac_dms: MacDms,
    bearer: Bearer,
    #[serde(default)]
    website_cookies: Vec<Cookie>,
    #[serde(default)]
    store_authentication_cookie: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct MacDms {
    adp_token: String,
    device_private_key: String,
}

#[derive(Deserialize)]
struct Bearer {
    access_token: String,
    refresh_token: String,
    expires_in: Seconds,
}

#[derive(Deserialize)]
struct Cookie {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Value")]
    value: String,
}

#[derive(Deserialize)]
struct Extensions {
    device_info: DeviceInfo,
    customer_info: CustomerInfo,
}

#[derive(Deserialize)]
struct DeviceInfo {
    #[serde(default)]
    device_name: Option<String>,
    device_serial_number: String,
    device_type: String,
}

#[derive(Deserialize)]
struct CustomerInfo {
    user_id: String,
    #[serde(default)]
    name: Option<String>,
}

impl RegisterResponse {
    fn into_credentials(self, session: &LoginSession) -> Result<Credentials> {
        let success = self.response.success;
        let device = success.extensions.device_info;
        // Amazon echoes the serial we registered; a mismatch means the
        // voucher key derivation would silently fail later.
        let serial = DeviceSerial::new(device.device_serial_number)?;
        if &serial != session.serial() {
            return Err(Error::Auth(
                "registered device serial differs from the requested serial".to_owned(),
            ));
        }
        let tokens = success.tokens;
        let cookies: BTreeMap<String, Secret> = tokens
            .website_cookies
            .into_iter()
            .map(|c| (c.name, Secret::new(c.value.replace('"', ""))))
            .collect();
        Ok(Credentials {
            marketplace: session.marketplace(),
            device_serial: serial,
            device_type: device.device_type,
            device_name: device.device_name,
            customer_id: success.extensions.customer_info.user_id,
            customer_name: success.extensions.customer_info.name,
            adp_token: Secret::new(tokens.mac_dms.adp_token),
            device_private_key: Secret::new(tokens.mac_dms.device_private_key),
            access_token: Secret::new(tokens.bearer.access_token),
            access_token_expires: Utc::now() + Duration::seconds(tokens.bearer.expires_in.0),
            refresh_token: Secret::new(tokens.bearer.refresh_token),
            website_cookies: cookies,
            store_authentication_cookie: tokens
                .store_authentication_cookie
                .map(|v| Secret::new(v.to_string())),
            activation_bytes: None,
        })
    }
}
