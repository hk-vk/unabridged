//! A string that never shows up in logs.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// A secret string (token, key, cookie). `Debug` and `Display` print
/// `<redacted>`; memory is zeroed on drop. Call [`Secret::expose`] at the one
/// place the raw value is needed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Wrap a value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The raw value. Do not log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_prints_value() {
        let s = Secret::new("Atnr|super-secret");
        assert_eq!(format!("{s:?} {s}"), "<redacted> <redacted>");
        assert_eq!(s.expose(), "Atnr|super-secret");
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"Atnr|super-secret\"");
    }
}
