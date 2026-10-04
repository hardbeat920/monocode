use std::{env, fs, path::PathBuf};
mod icu_build;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icu_build.rs");
    let target = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let suffix = match target.as_str() {
        "linux" => format!("_{}", icu_build::configure_linux()),
        "macos" => {
            println!("cargo:rustc-link-lib=icucore");
            String::new()
        }
        "windows" => return,
        _ => panic!("monocode-locale supports macOS, Windows and Linux"),
    };
    let mut bindings = String::from("unsafe extern \"C\" {\n");
    for (name, signature) in [
        (
            "ucol_open",
            "(locale: *const std::ffi::c_char, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        ("ucol_close", "(collator: *mut std::ffi::c_void)"),
        (
            "ucol_strcollUTF8",
            "(collator: *const std::ffi::c_void, a: *const std::ffi::c_char, a_len: i32, b: *const std::ffi::c_char, b_len: i32, status: *mut i32) -> i32",
        ),
        (
            "ucol_setAttribute",
            "(collator: *mut std::ffi::c_void, attribute: i32, value: i32, status: *mut i32)",
        ),
        (
            "ureldatefmt_open",
            "(locale: *const std::ffi::c_char, number_format: *mut std::ffi::c_void, width: i32, context: i32, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        ("ureldatefmt_close", "(formatter: *mut std::ffi::c_void)"),
        (
            "ureldatefmt_format",
            "(formatter: *const std::ffi::c_void, offset: f64, unit: i32, result: *mut u16, capacity: i32, status: *mut i32) -> i32",
        ),
        ("uloc_getDefault", "() -> *const std::ffi::c_char"),
        (
            "uloc_forLanguageTag",
            "(tag: *const std::ffi::c_char, locale: *mut std::ffi::c_char, capacity: i32, parsed: *mut i32, status: *mut i32) -> i32",
        ),
        (
            "uloc_getKeywordValue",
            "(locale: *const std::ffi::c_char, keyword: *const std::ffi::c_char, value: *mut std::ffi::c_char, capacity: i32, status: *mut i32) -> i32",
        ),
        (
            "uloc_setKeywordValue",
            "(keyword: *const std::ffi::c_char, value: *const std::ffi::c_char, locale: *mut std::ffi::c_char, capacity: i32, status: *mut i32) -> i32",
        ),
        (
            "uloc_acceptLanguage",
            "(result: *mut std::ffi::c_char, capacity: i32, accepted: *mut i32, requested: *const *const std::ffi::c_char, count: i32, available: *mut std::ffi::c_void, status: *mut i32) -> i32",
        ),
        (
            "uenum_openCharStringsEnumeration",
            "(strings: *const *const std::ffi::c_char, count: i32, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        (
            "uloc_openKeywords",
            "(locale: *const std::ffi::c_char, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        (
            "uenum_next",
            "(enumeration: *mut std::ffi::c_void, length: *mut i32, status: *mut i32) -> *const std::ffi::c_char",
        ),
        ("ucol_countAvailable", "() -> i32"),
        (
            "ucol_getAvailable",
            "(index: i32) -> *const std::ffi::c_char",
        ),
        ("udat_countAvailable", "() -> i32"),
        (
            "udat_getAvailable",
            "(index: i32) -> *const std::ffi::c_char",
        ),
        (
            "ucol_getKeywordValuesForLocale",
            "(keyword: *const std::ffi::c_char, locale: *const std::ffi::c_char, common: i8, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        (
            "unumsys_openByName",
            "(name: *const std::ffi::c_char, status: *mut i32) -> *mut std::ffi::c_void",
        ),
        ("unumsys_close", "(system: *mut std::ffi::c_void)"),
        (
            "unumsys_isAlgorithmic",
            "(system: *const std::ffi::c_void) -> i8",
        ),
        ("uenum_close", "(enumeration: *mut std::ffi::c_void)"),
    ] {
        bindings.push_str(&format!(
            "#[link_name = \"{name}{suffix}\"]\npub(super) fn {name}{signature};\n"
        ));
    }
    bindings.push_str("}\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("icu_locale.rs"),
        bindings,
    )
    .expect("Write ICU locale bindings");
}
