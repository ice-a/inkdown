fn main() {
    println!("cargo:rerun-if-changed=assets/windows.rc");
    println!("cargo:rerun-if-changed=assets/icon/inkdown.ico");

    // GPUI's Windows backend loads the window/taskbar icon from resource ID 1
    // (LoadImageW(.., PCWSTR(1), ..)), so the icon must be linked into the
    // executable at build time. Other platforms use their native packaging.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/windows.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to embed assets/windows.rc");
    }
}
