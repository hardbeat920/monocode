#!/bin/bash
set -euo pipefail
task_root="$HOME/Library/Caches/monocode-gpui-build"
source_root="$task_root/source"
run_root="$task_root/locale-retained-rust"
manifest="$source_root/src-tauri/Cargo.toml"
mkdir -p "$run_root"
export PATH="$task_root/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$task_root/retained-cargo"
export CARGO_TARGET_DIR="$task_root/retained-target"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_BUILD_JOBS=2
export DYLD_LIBRARY_PATH="$task_root/toolchain/lib"
cd "$source_root"
stage() {
    name=$1
    shift
    "$@" > "$run_root/$name.log" 2>&1
    echo "$name passed"
}
finish() {
    result=$?
    trap - EXIT
    set +e
    python3 "$task_root/retained-source-manifest.py" verify --root "$source_root" --manifest "$task_root/locale-retained-source-sha256.json" > "$run_root/retained-source-after.log" 2>&1
    retained_after=$?
    python3 "$task_root/final-source-manifest.py" verify --root "$source_root" --manifest "$task_root/final-source-manifest.json" > "$run_root/native-source-after.log" 2>&1
    native_after=$?
    shasum -a 256 src-tauri/Cargo.lock > "$run_root/lock-after.log"
    cmp -s "$run_root/Cargo.lock.before" src-tauri/Cargo.lock
    lock_result=$?
    echo "checks_result=$result retained_source_after=$retained_after native_source_after=$native_after lock_unchanged_result=$lock_result" | tee "$run_root/result.log"
    if test "$result" -eq 0 && test "$retained_after" -eq 0 && test "$native_after" -eq 0 && test "$lock_result" -eq 0; then
        exit 0
    fi
    exit 1
}
stage toolchain rustc --version --verbose
cp src-tauri/Cargo.lock "$run_root/Cargo.lock.before"
shasum -a 256 src-tauri/Cargo.lock > "$run_root/lock-before.log"
trap finish EXIT
stage retained-source-before python3 "$task_root/retained-source-manifest.py" verify --root "$source_root" --manifest "$task_root/locale-retained-source-sha256.json"
stage native-source-before python3 "$task_root/final-source-manifest.py" verify --root "$source_root" --manifest "$task_root/final-source-manifest.json"
stage fmt cargo fmt --check --manifest-path "$manifest"
stage clippy cargo clippy --manifest-path "$manifest" --all-targets --locked --offline -j 2 -- -D warnings
stage test cargo test --manifest-path "$manifest" --locked --offline -j 2
