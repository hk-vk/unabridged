//! Finds Amazon's sign-in redirect in the user's own browser, so nobody has to
//! copy it. Sign-in stays in their default browser; this only asks each
//! running browser, through Apple Events, for a tab on `/ap/maplanding`, and
//! closes that tab once read. macOS asks once per browser before allowing it.

#[cfg(target_os = "macos")]
use std::process::Command;

/// Browsers with an AppleScript tab model (`windows` → `tabs` → `URL`).
#[cfg(target_os = "macos")]
const BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.google.Chrome",
    "net.imput.helium",
    "dev.serafim.fast",
    "company.thebrowser.Browser",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "com.vivaldi.Vivaldi",
];

pub enum Scan {
    /// The full redirect URL, read from a tab that has now been closed.
    #[cfg(target_os = "macos")]
    Found(String),
    /// No running browser had the redirect open yet.
    NotFound,
    /// A browser is open but macOS refused access to it.
    #[cfg(target_os = "macos")]
    Denied,
}

/// Looks through every running supported browser once.
#[cfg(not(target_os = "macos"))]
pub fn find_redirect() -> Scan {
    Scan::NotFound
}

#[cfg(target_os = "macos")]
pub fn find_redirect() -> Scan {
    let running = running_bundle_ids();
    let mut denied = false;
    for id in BROWSERS
        .iter()
        .filter(|id| running.iter().any(|r| r == *id))
    {
        match scan_browser(id) {
            Ok(Some(url)) => return Scan::Found(url),
            Ok(None) => {}
            Err(true) => denied = true,
            Err(false) => {}
        }
    }
    if denied { Scan::Denied } else { Scan::NotFound }
}

/// Bundle ids of running apps, from `lsappinfo`, which needs no permission.
#[cfg(target_os = "macos")]
fn running_bundle_ids() -> Vec<String> {
    let Ok(output) = Command::new("/usr/bin/lsappinfo").arg("list").output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split("bundleID=\"").nth(1))
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect()
}

/// `Err(true)` means macOS denied Apple Events to this browser.
#[cfg(target_os = "macos")]
fn scan_browser(bundle_id: &str) -> Result<Option<String>, bool> {
    // The id has to be literal: `tabs` and `URL` only resolve against a named
    // application's dictionary when the script compiles.
    let script = format!(
        r#"tell application id "{bundle_id}"
	repeat with w in windows
		repeat with t in tabs of w
			try
				set u to URL of t
				if u contains "/ap/maplanding" and u contains "openid.oa2.authorization_code" then
					try
						close t
					end try
					return u
				end if
			end try
		end repeat
	end repeat
end tell
return """#
    );
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|_| false)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.contains("-1743"));
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!url.is_empty()).then_some(url))
}
