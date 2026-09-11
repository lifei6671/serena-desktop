fn main() {
    // A plain cargo release otherwise points the main window at devUrl.
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && std::env::var("DEP_TAURI_DEV").as_deref() == Ok("true")
    {
        panic!(
            "Production builds must embed the frontend. Run: npm run tauri -- build --no-bundle"
        );
    }
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Library test executables also reach the native dialog APIs. The
        // common-controls v6 activation context is required for TaskDialogIndirect.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        // Tauri already embeds the application manifest in resource.lib.
        // Disable linker-generated manifests only for application binaries;
        // library test executables still need the activation context above.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }
}
