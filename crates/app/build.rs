//! Build the macOS audio bridge.

fn main() {
    println!("cargo:rerun-if-changed=src/player.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/player.m")
            .flag("-fobjc-arc")
            .compile("better_audible_player");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
