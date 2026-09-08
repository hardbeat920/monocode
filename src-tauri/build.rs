use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // generate_context! embeds icons; cargo ignores them unless we watch here.
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=macos/Assets.car");
    println!("cargo:rerun-if-changed=tsnet/tsnet.go");
    println!("cargo:rerun-if-changed=tsnet/go.mod");
    println!("cargo:rerun-if-changed=tsnet/clangwrap-ios.sh");

    let target = env::var("TARGET").unwrap_or_default();
    if target == "aarch64-apple-ios" {
        build_tsnet_ios();
    }

    tauri_build::build()
}

fn find_go() -> PathBuf {
    if let Ok(path) = env::var("GO") {
        return PathBuf::from(path);
    }
    for candidate in ["/opt/homebrew/bin/go", "/usr/local/go/bin/go", "/usr/local/bin/go"]
    {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("go")
}

fn build_tsnet_ios() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let tsnet_dir = manifest_dir.join("tsnet");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let archive = out_dir.join("libmonocode_tsnet.a");
    let wrap = tsnet_dir.join("clangwrap-ios.sh");

    let go = find_go();
    let path = format!(
        "/opt/homebrew/bin:/usr/local/go/bin:/usr/bin:{}",
        env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(&go)
        .current_dir(&tsnet_dir)
        .env("PATH", &path)
        .env("CGO_ENABLED", "1")
        .env("GOOS", "ios")
        .env("GOARCH", "arm64")
        .env("CC", &wrap)
        .env("CGO_CFLAGS", "-fPIC")
        .args([
            "build",
            "-tags",
            "ios",
            "-buildmode=c-archive",
            "-ldflags=-w -s",
            "-o",
        ])
        .arg(&archive)
        .status()
        .expect("go is required to build the iPad tailnet node");
    if !status.success() {
        panic!("go build -buildmode=c-archive failed for src-tauri/tsnet");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=monocode_tsnet");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Security");
    println!("cargo:rustc-link-lib=framework=Network");
    println!("cargo:rustc-link-lib=resolv");

    // Xcode links libapp.a only. Copy the Go archive next to it and
    // force-load via OTHER_LDFLAGS so the Go runtime constructor is kept.
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let externals = manifest_dir
        .join("gen/apple/Externals/arm64")
        .join(&profile);
    let _ = std::fs::create_dir_all(&externals);
    let dest = externals.join("libmonocode_tsnet.a");
    if let Err(error) = std::fs::copy(&archive, &dest) {
        println!(
            "cargo:warning=could not copy {} to {}: {error}",
            archive.display(),
            dest.display()
        );
    }
}
