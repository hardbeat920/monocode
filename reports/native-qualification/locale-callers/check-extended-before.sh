#!/bin/bash
set -u
TASK_BUILD_ROOT="$HOME/Library/Caches/monocode-gpui-build"
export PATH="$TASK_BUILD_ROOT/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$TASK_BUILD_ROOT/cargo"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$TASK_BUILD_ROOT/toolchain/lib"
cd "$TASK_BUILD_ROOT/source" || exit 1
printf 'Extended locale caller BEFORE qualification\n'
date -u
rustc --version
cargo test --workspace --lib --bins --all-features --locked --offline --no-fail-fast -j 2 matches_intl_ > "$TASK_BUILD_ROOT/locale-extended-before.log" 2>&1
result=$?
cat "$TASK_BUILD_ROOT/locale-extended-before.log"
cargo test -p monocode-locale --all-features --locked --offline -j 2 > "$TASK_BUILD_ROOT/locale-native-tests.log" 2>&1
native_result=$?
cat "$TASK_BUILD_ROOT/locale-native-tests.log"
printf '\ncaller_before_result=%s\nnative_result=%s\n' "$result" "$native_result"
exit "$native_result"
