//! The design system's embedded assets, served to GPUI through [`AssetSource`].

use std::borrow::Cow;

use gpui_kit::{AssetSource, SharedString};

use crate::icon::Icon;

/// Serves the bundled icons. Applications with assets of their own wrap it
/// and fall back to [`Assets::get`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Assets;

impl Assets {
    /// The embedded bytes at `path`, if the design system owns it.
    pub fn get(path: &str) -> Option<&'static [u8]> {
        Icon::ALL
            .iter()
            .find(|icon| icon.path() == path)
            .map(|icon| icon.bytes())
    }

    /// Paths under `prefix`.
    pub fn list(prefix: &str) -> Vec<SharedString> {
        Icon::ALL
            .iter()
            .map(|icon| icon.path())
            .filter(|path| path.starts_with(prefix))
            .map(SharedString::from)
            .collect()
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Self::list(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_every_icon_and_nothing_else() {
        for icon in Icon::ALL {
            assert!(Assets::get(icon.path()).is_some());
        }
        assert!(Assets::get("icons/missing.svg").is_none());
        assert_eq!(Assets::list("icons/").len(), Icon::ALL.len());
    }
}
