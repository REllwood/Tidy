fn main() {
  // ScreenCaptureKit and Metal bring Swift code that loads the Swift runtime
  // by @rpath. macOS keeps it in /usr/lib/swift, which isn't searched by
  // default, so dev runs and tests abort at launch without this.
  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
  }
  tauri_build::build()
}
