use std::{env, fs, path::PathBuf};

mod dependency_guard;

fn main() {
    for name in [
        "MONOCODE_NATIVE_VERSION",
        "MONOCODE_NATIVE_UPDATER_ENDPOINT",
        "MONOCODE_NATIVE_UPDATER_PUBKEY",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let version = env::var("MONOCODE_NATIVE_VERSION")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=MONOCODE_NATIVE_VERSION={version}");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon(&manifest_dir);
    }
    dependency_guard::check(&manifest_dir);
}

/// GPUI loads the window and taskbar icon from resource ID 1 of the executable,
/// which Explorer and the installer shortcuts also show.
fn embed_windows_icon(manifest_dir: &std::path::Path) {
    let icon = manifest_dir.join("../../src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    assert!(
        icon.is_file(),
        "The Windows build requires src-tauri/icons/icon.ico"
    );
    // Not canonicalize: its \\?\ verbatim prefix is rejected by rc.exe.
    let icon = icon.to_string_lossy().replace('\\', "/");
    let rc = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("monocode.rc");
    fs::write(&rc, format!("1 ICON \"{icon}\"\n")).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
