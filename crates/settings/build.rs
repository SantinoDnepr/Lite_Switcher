//! Fills the "Details" tab of the file properties (version, description, copyright).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../icons/lite-switcher.ico")
        .set("FileDescription", "Lite Switcher - settings")
        .set("ProductName", "Lite Switcher")
        .set("InternalName", "lite-switcher-settings")
        .set("OriginalFilename", "lite-switcher-settings.exe")
        .set("LegalCopyright", "Copyright (c) 2026 SantinoDnepr. MIT License")
        .set_language(0x0409);
    if let Err(e) = res.compile() {
        // the build must not fail only because the resource compiler is missing
        println!("cargo:warning=version info was not embedded: {e}");
    }
}
