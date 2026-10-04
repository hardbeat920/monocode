#!/bin/bash
set -uo pipefail
TASK_BUILD_ROOT="$HOME/Library/Caches/monocode-gpui-build"
export PATH="$TASK_BUILD_ROOT/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$TASK_BUILD_ROOT/cargo"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$TASK_BUILD_ROOT/toolchain/lib"
cd "$TASK_BUILD_ROOT/source" || exit 2
date -u
rustc --version
cargo test --workspace --lib --bins --all-features --locked --offline -j 2 restored_typescript_ > "$TASK_BUILD_ROOT/native-receipt-final-after.log" 2>&1
receipt_result=$?
cargo test --workspace --lib --bins --all-features --locked --offline -j 2 receipt_signature_tests > "$TASK_BUILD_ROOT/native-receipt-final-pure.log" 2>&1
pure_result=$?
cargo test --workspace --lib --bins --all-features --locked --offline --no-run --message-format=json -j 2 > "$TASK_BUILD_ROOT/native-receipt-final-artifacts.jsonl" 2> "$TASK_BUILD_ROOT/native-receipt-final-artifacts.log"
compile_result=$?
engine_result=2
if test "$compile_result" -eq 0; then
    python3 "$TASK_BUILD_ROOT/run-qualified-engine.py" "$TASK_BUILD_ROOT/native-receipt-final-artifacts.jsonl" > "$TASK_BUILD_ROOT/native-receipt-final-engine-suite.log" 2>&1
    engine_result=$?
fi
cargo clippy --workspace --all-targets --all-features --locked --offline -j 2 -- -D warnings > "$TASK_BUILD_ROOT/native-receipt-final-clippy.log" 2>&1
clippy_result=$?
cargo fmt --all -- --check > "$TASK_BUILD_ROOT/native-receipt-final-fmt.log" 2>&1
fmt_result=$?
echo "receipt_result=$receipt_result pure_result=$pure_result compile_result=$compile_result engine_result=$engine_result clippy_result=$clippy_result fmt_result=$fmt_result"
test "$receipt_result" -eq 0 && test "$pure_result" -eq 0 && test "$compile_result" -eq 0 && test "$engine_result" -eq 0 && test "$clippy_result" -eq 0 && test "$fmt_result" -eq 0
