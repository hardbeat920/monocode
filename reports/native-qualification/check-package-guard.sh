#!/bin/sh
set -eu

task_root="$HOME/Library/Caches/monocode-gpui-build"
export PATH="$task_root/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$task_root/cargo"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$task_root/toolchain/lib"
cd "$task_root/source"

cargo test -p monocode-package --locked --offline -j 2
cargo clippy -p monocode-package --all-targets --locked --offline -j 2 -- -D warnings
cargo build -p monocode-package --locked --offline -j 2
