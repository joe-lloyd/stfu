fn main() {
    // ScreenCaptureKit only captures app audio from macOS 13, and is missing before 12.3. Link it
    // weakly so the app still starts on older systems; meeting transcripts check the OS first.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,ScreenCaptureKit");
    }
    tauri_build::build()
}
