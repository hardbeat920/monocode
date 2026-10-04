#!/bin/sh
# Run the search qualification on hometop from a fresh copy of the source archive.
set -eu
archive=$1
task_root="$HOME/Library/Caches/monocode-gpui-build"
source="$task_root/search-final-source"
output="$task_root/search-final-output"
rm -rf "$source" "$output"
mkdir -p "$source" "$output"
tar -xmzf "$archive" -C "$source"
cd "$source"
export PATH="$task_root/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$task_root/cargo"
export CARGO_TARGET_DIR="$task_root/source/target"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export DYLD_LIBRARY_PATH="$task_root/toolchain/lib"
{ sw_vers; uname -a; rustc -Vv; cargo -V; cargo clippy -V; python3 -V; } > "$output/platform.log" 2>&1
shasum -a 256 "$archive" > "$output/archive-sha256.txt"
cargo test --workspace --lib --bins --all-features --locked -j4 --no-run --message-format=json > "$output/compile.log" 2> "$output/compile-stderr.log"
python3 reports/native-qualification/javascript-search/check-after.py --source "$source" --output "$output" --compile-log "$output/compile.log"
cargo clippy --workspace --all-targets --all-features --locked -j4 -- -D warnings > "$output/clippy.log" 2>&1
echo complete
