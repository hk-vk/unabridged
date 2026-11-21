use std::path::PathBuf;

fn environment(name: &str) -> anyhow::Result<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("{name} directory not found"))
}

pub fn support() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let root = environment("HOME")?.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let root = environment("APPDATA")?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = environment("XDG_DATA_HOME")
        .or_else(|_| environment("HOME").map(|p| p.join(".local/share")))?;
    Ok(root.join("Better Audible"))
}

pub fn cache() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let root = environment("HOME")?.join("Library/Caches/Better Audible");
    #[cfg(target_os = "windows")]
    let root = environment("LOCALAPPDATA")?.join("Better Audible/Caches");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = environment("XDG_CACHE_HOME")
        .or_else(|_| environment("HOME").map(|p| p.join(".cache")))?
        .join("Better Audible");
    Ok(root)
}
