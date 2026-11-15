//! Audible marketplaces and the hosts that belong to each one.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// An Audible marketplace (store front).
///
/// Values come from `audible.localization.LOCALE_TEMPLATES` in the Python
/// `audible` library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marketplace {
    /// audible.com
    UnitedStates,
    /// audible.co.uk
    UnitedKingdom,
    /// audible.de
    Germany,
    /// audible.fr
    France,
    /// audible.ca
    Canada,
    /// audible.it
    Italy,
    /// audible.com.au
    Australia,
    /// audible.in
    India,
    /// audible.co.jp
    Japan,
    /// audible.es
    Spain,
    /// audible.com.br
    Brazil,
}

impl Marketplace {
    /// Every supported marketplace, for pickers.
    pub const ALL: [Marketplace; 11] = [
        Self::UnitedStates,
        Self::UnitedKingdom,
        Self::Germany,
        Self::France,
        Self::Canada,
        Self::Italy,
        Self::Australia,
        Self::India,
        Self::Japan,
        Self::Spain,
        Self::Brazil,
    ];

    /// Short country code used in the login `assoc_handle` (`us`, `uk`, ...).
    pub fn country_code(self) -> &'static str {
        match self {
            Self::UnitedStates => "us",
            Self::UnitedKingdom => "uk",
            Self::Germany => "de",
            Self::France => "fr",
            Self::Canada => "ca",
            Self::Italy => "it",
            Self::Australia => "au",
            Self::India => "in",
            Self::Japan => "jp",
            Self::Spain => "es",
            Self::Brazil => "br",
        }
    }

    /// Top-level domain shared by `amazon.<tld>` and `audible.<tld>`.
    pub fn domain(self) -> &'static str {
        match self {
            Self::UnitedStates => "com",
            Self::UnitedKingdom => "co.uk",
            Self::Germany => "de",
            Self::France => "fr",
            Self::Canada => "ca",
            Self::Italy => "it",
            Self::Australia => "com.au",
            Self::India => "in",
            Self::Japan => "co.jp",
            Self::Spain => "es",
            Self::Brazil => "com.br",
        }
    }

    /// Amazon marketplace identifier.
    pub fn marketplace_id(self) -> &'static str {
        match self {
            Self::UnitedStates => "AF2M0KC94RCEA",
            Self::UnitedKingdom => "A2I9A3Q2GNFNGQ",
            Self::Germany => "AN7V1F1VY261K",
            Self::France => "A2728XDNODOQ8T",
            Self::Canada => "A2CQZ5RBY40XE",
            Self::Italy => "A2N7FU2W2BU2ZC",
            Self::Australia => "AN7EY7DTAW63G",
            Self::India => "AJO3FBRUE6J4S",
            Self::Japan => "A1QAP3MOU4173J",
            Self::Spain => "ALMIKO4SZCSAR",
            Self::Brazil => "A10J1VAYUDTYRN",
        }
    }

    /// Human-readable store name.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::UnitedStates => "United States",
            Self::UnitedKingdom => "United Kingdom",
            Self::Germany => "Germany",
            Self::France => "France",
            Self::Canada => "Canada",
            Self::Italy => "Italy",
            Self::Australia => "Australia",
            Self::India => "India",
            Self::Japan => "Japan",
            Self::Spain => "Spain",
            Self::Brazil => "Brazil",
        }
    }

    /// The production hosts for this marketplace.
    pub fn endpoints(self) -> Endpoints {
        let tld = self.domain();
        Endpoints {
            api: format!("https://api.audible.{tld}"),
            amazon_api: format!("https://api.amazon.{tld}"),
            amazon_www: format!("https://www.amazon.{tld}"),
            audible_www: format!("https://www.audible.{tld}"),
            // Activation bytes are always fetched from audible.com.
            activation: "https://www.audible.com".to_owned(),
        }
    }
}

impl fmt::Display for Marketplace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for Marketplace {
    type Err = Error;

    /// Accepts a country code (`us`, `uk`, `de`) or a domain (`com`, `co.uk`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let needle = s.trim().trim_start_matches('.').to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|m| m.country_code() == needle || m.domain() == needle)
            .ok_or_else(|| Error::InvalidArgument(format!("unknown marketplace `{s}`")))
    }
}

/// Base URLs (scheme + host, no trailing slash) used by the client.
///
/// Production values come from [`Marketplace::endpoints`]; tests point every
/// field at a local mock server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// `https://api.audible.<tld>`: library, catalog, content, positions.
    pub api: String,
    /// `https://api.amazon.<tld>`: device registration and token refresh.
    pub amazon_api: String,
    /// `https://www.amazon.<tld>`: the OAuth sign-in page.
    pub amazon_www: String,
    /// `https://www.audible.<tld>`: AAX downloads.
    pub audible_www: String,
    /// Host serving the activation blob (always `www.audible.com`).
    pub activation: String,
}

impl Endpoints {
    /// Point every endpoint at one base URL (for tests and proxies).
    pub fn all(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        Self {
            api: base.clone(),
            amazon_api: base.clone(),
            amazon_www: base.clone(),
            audible_www: base.clone(),
            activation: base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_country_codes_and_domains() {
        assert_eq!(
            "us".parse::<Marketplace>().ok(),
            Some(Marketplace::UnitedStates)
        );
        assert_eq!(
            "co.uk".parse::<Marketplace>().ok(),
            Some(Marketplace::UnitedKingdom)
        );
        assert_eq!(
            ".de".parse::<Marketplace>().ok(),
            Some(Marketplace::Germany)
        );
        assert!("xx".parse::<Marketplace>().is_err());
    }

    #[test]
    fn endpoints_use_marketplace_domain() {
        let e = Marketplace::Japan.endpoints();
        assert_eq!(e.api, "https://api.audible.co.jp");
        assert_eq!(e.amazon_api, "https://api.amazon.co.jp");
        assert_eq!(e.activation, "https://www.audible.com");
    }

    #[test]
    fn serde_round_trip() {
        let json = serde_json::to_string(&Marketplace::UnitedKingdom).ok();
        assert_eq!(json.as_deref(), Some("\"united_kingdom\""));
    }
}
