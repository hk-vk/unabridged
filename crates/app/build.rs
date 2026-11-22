//! Build the macOS audio bridge and the read-along transcription helper.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=../../assets/windows/AppIcon.ico");
        embed_resource::compile("app.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
    println!("cargo:rerun-if-changed=src/player.m");
    println!("cargo:rerun-if-changed=src/window.m");
    println!("cargo:rerun-if-changed=src/updater.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/player.m")
            .file("src/window.m")
            .file("src/updater.m")
            .flag("-fobjc-arc")
            // The app's floor (LSMinimumSystemVersion): newer APIs then need
            // an availability check and are weak-linked, instead of being
            // required at launch.
            .flag("-mmacosx-version-min=14.0")
            .flag("-Werror=unguarded-availability-new")
            .compile("better_audible_player");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=QuartzCore");
        build_transcriber();
    }
}

/// Compiles `ba-transcribe` (Apple SpeechAnalyzer, macOS 26+) next to the app
/// binary. Read along reports itself unavailable when this is missing.
fn build_transcriber() {
    println!("cargo:rerun-if-changed=src/transcribe.swift");
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(std::path::PathBuf::from) else {
        return;
    };
    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out.
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x86_64",
        _ => "arm64",
    };
    let status = std::process::Command::new("xcrun")
        .args(["swiftc", "-O", "-target"])
        .arg(format!("{arch}-apple-macos26.0"))
        .arg("src/transcribe.swift")
        .arg("-o")
        .arg(profile_dir.join("ba-transcribe"))
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("cargo:warning=Read-along helper (ba-transcribe) was not built");
    }
}
