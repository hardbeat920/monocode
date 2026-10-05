#!/usr/bin/env bash
set -euo pipefail

# Run only after the coordinator releases hometop's Cargo work.
task_root=${MONOCODE_QUALIFICATION_ROOT:-"$HOME/Library/Caches/monocode-gpui-build"}
source_root=${MONOCODE_RETAINED_SOURCE:-"$task_root/source"}
legacy_cargo="$task_root/retained-cargo"
legacy_target="$task_root/retained-target"
run_root="$task_root/retained-rust-$(date -u +%Y%m%dT%H%M%SZ)"
manifest="$source_root/src-tauri/Cargo.toml"

test -f "$manifest"
test -x "$task_root/toolchain/bin/cargo"
mkdir -p "$legacy_cargo" "$run_root"

# Clone the task cache into a separate Cargo home. APFS clones avoid copying
# its cached archive contents. No user Cargo configuration is read or changed.
if test ! -d "$legacy_cargo/registry" && test -d "$task_root/cargo/registry"; then
    cp -cR "$task_root/cargo/registry" "$legacy_cargo/registry"
fi

export PATH="$task_root/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$legacy_cargo"
export CARGO_TARGET_DIR="$legacy_target"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_BUILD_JOBS=2
export DYLD_LIBRARY_PATH="$task_root/toolchain/lib"
cd "$source_root"

stage() {
    local name=$1
    shift
    printf 'stage %s\n' "$name"
    "$@" 2>&1 | tee "$run_root/$name.log"
}

printf 'retained source %s\nlogs %s\n' "$source_root" "$run_root"
stage toolchain rustc --version --verbose
shasum -a 256 Cargo.toml src-tauri/Cargo.toml src-tauri/Cargo.lock \
    crates/{platform,process,git,store,terminal,integrations,remote}/Cargo.toml \
    > "$run_root/source-hashes.txt"
cp src-tauri/Cargo.lock "$run_root/Cargo.lock.before"

stage fmt cargo fmt --check --manifest-path "$manifest"
if ! stage fetch-offline cargo fetch --offline --manifest-path "$manifest"; then
    stage fetch cargo fetch --manifest-path "$manifest"
fi
cp src-tauri/Cargo.lock "$run_root/Cargo.lock.after"
if cmp -s "$run_root/Cargo.lock.before" "$run_root/Cargo.lock.after"; then
    printf 'retained lockfile unchanged\n' | tee "$run_root/lock-status.txt"
else
    printf 'retained lockfile refreshed only in the remote task source\n' \
        | tee "$run_root/lock-status.txt"
    diff -u "$run_root/Cargo.lock.before" "$run_root/Cargo.lock.after" \
        > "$run_root/lock.diff" || test "$?" -eq 1
fi
stage clippy cargo clippy --manifest-path "$manifest" --all-targets --locked -- -D warnings
stage test cargo test --manifest-path "$manifest" --locked
printf 'retained Rust checks passed\n' | tee "$run_root/result.txt"
