use std::{env, fs, path::PathBuf};

#[path = "../locale/icu_build.rs"]
mod icu_build;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../locale/icu_build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    let major = icu_build::configure_linux();
    // ICU's public C ABI renames symbols with its major version. Match the
    // library selected by pkg-config instead of pinning a distribution's ICU.
    let mut bindings = String::from("unsafe extern \"C\" {\n");
    for (name, signature) in [
        (
            "udatpg_open",
            "(locale: *const std::ffi::c_char, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        ("udatpg_close", "(generator: *mut std::ffi::c_void)"),
        (
            "udatpg_getBestPattern",
            "(generator: *mut std::ffi::c_void, skeleton: *const u16, length: i32, pattern: *mut u16, capacity: i32, status: *mut i32) -> i32",
        ),
        (
            "udat_open",
            "(time_style: i32, date_style: i32, locale: *const std::ffi::c_char, zone: *const u16, zone_length: i32, pattern: *const u16, pattern_length: i32, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        ("udat_close", "(formatter: *mut std::ffi::c_void)"),
        (
            "udat_format",
            "(formatter: *const std::ffi::c_void, epoch_ms: f64, result: *mut u16, capacity: i32, field_position: *mut std::ffi::c_void, status: *mut i32) -> i32",
        ),
    ] {
        bindings.push_str(&format!(
            "#[link_name = \"{name}_{major}\"]\nfn {name}{signature};\n"
        ));
    }
    bindings.push_str("}\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("icu_date.rs"),
        bindings,
    )
    .expect("Write ICU date bindings");
}
