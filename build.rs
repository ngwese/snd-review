// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/app-icon.ico");
    println!("cargo:rerun-if-changed=assets/logo/04-bands.svg");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let icon = std::path::Path::new(&manifest_dir)
        .join("assets")
        .join("app-icon")
        .join("app-icon.ico");
    if !icon.is_file() {
        panic!(
            "missing {}; run python script/generate-app-icon.py",
            icon.display()
        );
    }

    let icon_escaped = icon.display().to_string().replace('\\', "/");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let rc_path = std::path::Path::new(&out_dir).join("app_icon.rc");
    std::fs::write(&rc_path, format!("1 ICON \"{icon_escaped}\"\n")).expect("write app_icon.rc");

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("embed Windows app icon");
}
