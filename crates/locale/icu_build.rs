use std::{env, path::PathBuf, process::Command};

fn pkg_config(args: &[&str]) -> String {
    let tool = env::var_os("PKG_CONFIG").unwrap_or_else(|| "pkg-config".into());
    let output = Command::new(tool)
        .args(args)
        .arg("icu-i18n")
        .output()
        .expect("Run pkg-config for ICU. Install the native Linux dependencies first.");
    assert!(
        output.status.success(),
        "ICU development files are required on Linux: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("pkg-config output must be UTF-8")
}

/// Link the ICU installation selected by pkg-config and return its ABI suffix.
pub fn configure_linux() -> String {
    for key in [
        "PKG_CONFIG",
        "PKG_CONFIG_PATH",
        "PKG_CONFIG_LIBDIR",
        "PKG_CONFIG_SYSROOT_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let version = pkg_config(&["--modversion"]);
    let major = version.trim().split('.').next().unwrap();
    assert!(
        !major.is_empty() && major.chars().all(|ch| ch.is_ascii_digit()),
        "Invalid ICU version"
    );
    let libdir = pkg_config(&["--variable=libdir"]);
    let static_icu = ["icui18n", "icuuc", "icudata"].iter().all(|library| {
        PathBuf::from(libdir.trim())
            .join(format!("lib{library}.a"))
            .is_file()
    });
    if static_icu {
        println!("cargo:rustc-link-search=native={}", libdir.trim());
    }
    for flag in pkg_config(if static_icu {
        &["--libs", "--static"]
    } else {
        &["--libs"]
    })
    .split_whitespace()
    {
        if let Some(path) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={path}");
        } else if let Some(library) = flag.strip_prefix("-l") {
            if static_icu && matches!(library, "icui18n" | "icuuc" | "icudata") {
                println!("cargo:rustc-link-lib=static={library}");
            } else {
                println!("cargo:rustc-link-lib={library}");
            }
        } else {
            println!("cargo:rustc-link-arg={flag}");
        }
    }
    if static_icu {
        // ICU's C API calls C++ code. Include both the implementation and locale
        // data in portable binaries that do not have an ICU runtime installed.
        println!("cargo:rustc-link-lib=stdc++");
    }
    major.to_owned()
}
