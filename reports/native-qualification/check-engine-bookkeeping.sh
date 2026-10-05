#!/bin/sh
set -u

task_root="$HOME/Library/Caches/monocode-gpui-build"
export PATH="$task_root/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$task_root/cargo"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$task_root/toolchain/lib"
cd "$task_root/source" || exit 1

cargo test --workspace --lib --bins --all-features --locked --offline -j 2 removes_a_draft_and_discards_a_draft_only_session
draft_result=$?
cargo test --workspace --lib --bins --all-features --locked --offline -j 2 releasing_a_lead_invalidates_a_pending_worker_read
load_result=$?
printf 'draft_result=%s\nload_result=%s\n' "$draft_result" "$load_result"
if [ "$draft_result" -ne 0 ] || [ "$load_result" -ne 0 ]; then
    exit 1
fi
