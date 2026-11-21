//! Application data shared by accounts and downloaded speech models.
use std::path::PathBuf;

/// Persistent application support directory, independent of the active account.
pub fn support() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let (variable, suffix) = ("APPDATA", "Better Audible");
    #[cfg(target_os = "macos")]
    let (variable, suffix) = ("HOME", "Library/Application Support/Better Audible");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (variable, suffix) = ("HOME", ".local/share/Better Audible");
    let home = std::env::var_os(variable)
        .ok_or_else(|| anyhow::anyhow!("Application data directory not found"))?;
    Ok(PathBuf::from(home).join(suffix))
}
