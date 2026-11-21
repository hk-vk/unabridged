use std::{io, path::Path, process::Command};

pub const SHOW_LABEL: &str = if cfg!(target_os = "windows") {
    "Show in Explorer"
} else if cfg!(target_os = "macos") {
    "Show in Finder"
} else {
    "Show in file manager"
};
pub const NAME: &str = if cfg!(target_os = "windows") {
    "File Explorer"
} else if cfg!(target_os = "macos") {
    "Finder"
} else {
    "file manager"
};

pub fn open(path: &Path, select: bool) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        if select {
            command.arg("-R");
        }
        command.arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer.exe");
        if select {
            let mut argument = std::ffi::OsString::from("/select,");
            argument.push(path);
            command.arg(argument);
        } else {
            command.arg(path);
        }
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(if select {
            path.parent().unwrap_or(path)
        } else {
            path
        });
        command
    };
    command.spawn().map(|_| ())
}
