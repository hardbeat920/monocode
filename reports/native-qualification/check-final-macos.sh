#!/bin/sh
set -eu

task_root="$HOME/Library/Caches/monocode-gpui-build"
export PATH="$task_root/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$task_root/cargo"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$task_root/toolchain/lib"
cd "$task_root/source"
exec python3 "$task_root/final-native-check.py" \
    --source "$task_root/source" \
    --output "$task_root/final-macos" \
    --manifest "$task_root/final-source-manifest.json" \
    --manifest-helper "$task_root/final-source-manifest.py"
