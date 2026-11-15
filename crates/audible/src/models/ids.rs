//! Newtype identifiers.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Amazon Standard Identification Number of a title (e.g. `B002V5D7RU`).
///
/// ASINs are ASCII alphanumeric; validating on construction keeps them safe to
/// interpolate into URL paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Asin(String);

impl Asin {
    /// Validate and wrap an ASIN.
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(Error::InvalidArgument(format!("invalid ASIN `{value}`")));
        }
        Ok(Self(value))
    }

    /// The ASIN as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Asin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Asin {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for Asin {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Asin> for String {
    fn from(value: Asin) -> Self {
        value.0
    }
}

impl AsRef<str> for Asin {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Catalog category (genre) identifier. Audible returns these as numeric
/// strings; they are kept as strings to avoid overflow surprises.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CategoryId(pub String);

impl fmt::Display for CategoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_asins() {
        assert!(Asin::new("B002V5D7RU").is_ok());
        assert!(Asin::new("").is_err());
        assert!(Asin::new("../etc").is_err());
        let parsed: Result<Asin, _> = serde_json::from_str("\"B0 X\"");
        assert!(parsed.is_err());
    }
}
