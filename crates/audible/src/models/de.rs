//! Serde helpers for Audible's loose JSON (fields may be `null` or absent).

use serde::{Deserialize, Deserializer};

/// Deserialize `null` as `T::default()`. Combine with `#[serde(default)]`
/// so a missing field also yields the default.
pub(crate) fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
