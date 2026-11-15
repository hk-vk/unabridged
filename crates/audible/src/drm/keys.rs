//! Key material for Audible files.

use std::fmt;
use std::str::FromStr;

use crate::error::Error;

/// The account's 4 activation bytes (AAX), usually shown as 8 hex chars.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ActivationBytes(pub [u8; 4]);

impl ActivationBytes {
    /// Lower-case hex form, as ffmpeg's `-activation_bytes` expects.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl FromStr for ActivationBytes {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s.trim())
            .ok()
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .ok_or_else(|| {
                Error::InvalidArgument("activation bytes must be 8 hex characters".into())
            })?;
        Ok(Self(bytes))
    }
}

impl fmt::Debug for ActivationBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationBytes(<redacted>)")
    }
}

/// How to decrypt one downloaded file.
#[derive(Clone, PartialEq, Eq)]
pub enum DecryptionKey {
    /// AAX: derive the file key from activation bytes and the `adrm` box.
    Aax(ActivationBytes),
    /// AAXC: key and IV from the license voucher.
    Aaxc {
        /// AES-128 key.
        key: [u8; 16],
        /// AES-128 IV.
        iv: [u8; 16],
    },
}

impl fmt::Debug for DecryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aax(_) => f.write_str("DecryptionKey::Aax(<redacted>)"),
            Self::Aaxc { .. } => f.write_str("DecryptionKey::Aaxc(<redacted>)"),
        }
    }
}
