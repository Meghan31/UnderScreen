fn main() {
    // Link Apple's Vision framework for the OCR pipeline.
    // VNRecognizeTextRequest, VNImageRequestHandler, etc. live here.
    // This is a macOS system framework — no installation required.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=Vision");
    }

    tauri_build::build()
}
