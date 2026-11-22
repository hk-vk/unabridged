//! Suggest an Audible marketplace from the local OS locale, without a network lookup.
use audible::Marketplace;

fn marketplace_for_region(region: &str) -> Option<Marketplace> {
    match region.to_ascii_uppercase().as_str() {
        "US" => Some(Marketplace::UnitedStates),
        "GB" | "UK" => Some(Marketplace::UnitedKingdom),
        "DE" => Some(Marketplace::Germany),
        "FR" => Some(Marketplace::France),
        "CA" => Some(Marketplace::Canada),
        "IT" => Some(Marketplace::Italy),
        "AU" => Some(Marketplace::Australia),
        "IN" => Some(Marketplace::India),
        "JP" => Some(Marketplace::Japan),
        "ES" => Some(Marketplace::Spain),
        "BR" => Some(Marketplace::Brazil),
        _ => None,
    }
}

fn override_region(value: &str) -> Option<&str> {
    let value = value.trim();
    (value.len() >= 2 && value.as_bytes()[..2].iter().all(u8::is_ascii_alphabetic))
        .then(|| &value[..2])
}

fn from_locale(locale: &str) -> Option<Marketplace> {
    // macOS may store a preferred regional override, e.g. en_US@rg=inzzzz.
    if let Some((_, modifiers)) = locale.split_once('@') {
        for entry in modifiers.split(';') {
            if let Some(value) = entry.trim().strip_prefix("rg=") {
                return override_region(value).and_then(marketplace_for_region);
            }
        }
    }
    let base = locale.split(['@', '.']).next().unwrap_or(locale);
    let parts: Vec<_> = base.split(['_', '-']).collect();
    if let Some(index) = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("rg"))
    {
        return parts
            .get(index + 1)
            .and_then(|value| override_region(value))
            .and_then(marketplace_for_region);
    }
    parts
        .iter()
        .skip(1)
        .find(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .and_then(|region| marketplace_for_region(region))
}

pub fn suggested_marketplace() -> Option<Marketplace> {
    #[cfg(target_os = "windows")]
    {
        let mut locale = [0u16; 85];
        let length =
            unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut locale) };
        if length > 1
            && let Some(marketplace) =
                from_locale(&String::from_utf16_lossy(&locale[..length as usize - 1]))
        {
            return Some(marketplace);
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        && output.status.success()
        && let Ok(locale) = std::str::from_utf8(&output.stdout)
    {
        return from_locale(locale.trim());
    }
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .and_then(|locale| from_locale(&locale))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_region_and_overrides_select_the_correct_store() {
        assert_eq!(from_locale("en_US@rg=inzzzz"), Some(Marketplace::India));
        assert_eq!(from_locale("en-GB"), Some(Marketplace::UnitedKingdom));
        assert_eq!(
            from_locale("en-US-u-rg-auzzzz"),
            Some(Marketplace::Australia)
        );
        assert_eq!(from_locale("de_DE.UTF-8"), Some(Marketplace::Germany));
        assert_eq!(from_locale("en_US@rg=hkzzzz"), None);
        assert_eq!(from_locale("en"), None);
    }
}
