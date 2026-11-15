//! Sign-in: authorization URL with PKCE, device registration, token refresh.
//!
//! Flow for the UI:
//! 1. `let session = LoginSession::new(marketplace);`
//! 2. Load `session.authorization_url()?` in a web view with
//!    [`LOGIN_USER_AGENT`] and [`LoginSession::initial_cookies`].
//! 3. When a navigation satisfies [`LoginSession::is_redirect`], stop and call
//!    [`register_device`] (or `Audible::sign_in`) with that URL.

mod device;
mod login;
mod pkce;
mod register;

pub use device::{DeviceProfile, DeviceSerial, SERIAL_LEN};
pub use login::{LOGIN_USER_AGENT, LoginCookie, LoginSession};
pub use pkce::CodeVerifier;
pub use register::{AccessToken, deregister_device, refresh_access_token, register_device};
