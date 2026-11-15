//! The device identity this library registers as.
//!
//! Audible ties licenses to a registered device. We register as the official
//! "Audible for iPhone" app using the same profile as mkb79's `audible` Python
//! library (v0.13.1, `_device.py` and `register.py`).
//!
//! Serial numbers: Libation 14.2.0 fixed `License Denied` / `CustomerThrottled`
//! failures caused by registering devices "with a serial number twice the
//! expected length". The usual way to get there is hex-encoding a serial that
//! is then hex-encoded again for the client id. [`DeviceSerial`] therefore
//! only accepts exactly 40 ASCII alphanumerics and is hex-encoded exactly
//! once, in [`DeviceProfile::client_id`].

use std::fmt;

use rand::Rng;
use rand::distributions::Alphanumeric;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Length of a device serial, per `audible.login.build_device_serial`.
pub const SERIAL_LEN: usize = 40;

/// Static description of the emulated app/device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Amazon device type id.
    pub device_type: &'static str,
    /// Audible app version string.
    pub app_version: &'static str,
    /// Audible app build number.
    pub software_version: &'static str,
    /// iOS version.
    pub os_version: &'static str,
    /// Device model.
    pub device_model: &'static str,
    /// App bundle id (used in the login `map-md` cookie).
    pub bundle_id: &'static str,
    /// Device name template; Amazon substitutes the placeholders.
    pub device_name: &'static str,
}

impl DeviceProfile {
    /// Audible for iPhone, matching the Python `audible` library 0.13.1.
    pub const IPHONE: DeviceProfile = DeviceProfile {
        device_type: "A2CZJZGLK2JJVM",
        app_version: "4.60",
        software_version: "46000856",
        os_version: "26.2",
        device_model: "iPhone",
        bundle_id: "com.audible.iphone",
        device_name: "%FIRST_NAME%%FIRST_NAME_POSSESSIVE_STRING%%DUPE_STRATEGY_1ST%Audible for iPhone",
    };

    /// OAuth client id: hex of `"<serial>#<device_type>"`.
    pub fn client_id(&self, serial: &DeviceSerial) -> String {
        hex::encode(format!("{}#{}", serial.as_str(), self.device_type))
    }
}

/// A 40-character alphanumeric device serial, generated once per sign-in and
/// stored with the credentials (it is needed again to decrypt AAXC vouchers).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceSerial(String);

impl DeviceSerial {
    /// Generate a fresh random serial from the OS RNG.
    pub fn generate() -> Self {
        let serial: String = rand::rngs::OsRng
            .sample_iter(&Alphanumeric)
            .take(SERIAL_LEN)
            .map(char::from)
            .collect();
        Self(serial)
    }

    /// Validate an existing serial.
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        if value.len() != SERIAL_LEN || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(Error::InvalidArgument(format!(
                "device serial must be {SERIAL_LEN} ASCII alphanumerics, got {} characters",
                value.len()
            )));
        }
        Ok(Self(value))
    }

    /// The serial.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DeviceSerial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The serial is an identifier, not a secret, but keep logs short.
        write!(f, "DeviceSerial({}…)", &self.0[..4.min(self.0.len())])
    }
}

impl TryFrom<String> for DeviceSerial {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DeviceSerial> for String {
    fn from(value: DeviceSerial) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn generated_serial_has_expected_shape() {
        let s = DeviceSerial::generate();
        assert_eq!(s.as_str().len(), SERIAL_LEN);
        assert!(s.as_str().bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_ne!(s, DeviceSerial::generate());
    }

    #[test]
    fn client_id_matches_python_reference() {
        // audible.login.build_client_id("A"*40) == ("A"*40 + "#A2CZJZGLK2JJVM").encode().hex()
        let serial = DeviceSerial::new("A".repeat(40)).unwrap();
        let id = DeviceProfile::IPHONE.client_id(&serial);
        assert_eq!(
            id,
            format!("{}234132435a4a5a474c4b324a4a564d", "41".repeat(40))
        );
        assert_eq!(id.len(), (40 + 15) * 2);
    }

    #[test]
    fn rejects_double_length_or_hex_encoded_serials() {
        let raw = DeviceSerial::generate();
        assert!(DeviceSerial::new(hex::encode(raw.as_str())).is_err());
        assert!(DeviceSerial::new("short").is_err());
        assert!(DeviceSerial::new("a".repeat(39) + "-").is_err());
    }

    proptest! {
        #[test]
        fn client_id_round_trips(seed in "[A-Za-z0-9]{40}") {
            let serial = DeviceSerial::new(seed.clone()).unwrap();
            let decoded = hex::decode(DeviceProfile::IPHONE.client_id(&serial)).unwrap();
            prop_assert_eq!(String::from_utf8(decoded).unwrap(), format!("{seed}#A2CZJZGLK2JJVM"));
        }
    }
}
