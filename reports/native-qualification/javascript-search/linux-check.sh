#!/bin/bash
# Run the search qualification on QRK-GLUON's WSL Ubuntu from a fresh copy of the source archive.
set -euo pipefail
archive=$1
root=/home/niost/monocode-search-qualification
source=$root/source
output=$root/output
rm -rf "$source" "$output"
mkdir -p "$source" "$output"
tar -xmzf "$archive" -C "$source"
cd "$source"
export PATH="/home/niost/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=/home/niost/monocode-gpui-qualification/target
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
{ uname -a; cat /etc/os-release; rustc -Vv; cargo -V; python3 -V; } > "$output/platform.log" 2>&1
sha256sum "$archive" > "$output/archive-sha256.txt"
cargo test --workspace --lib --bins --all-features --locked -j4 --no-run --message-format=json > "$output/compile.log" 2> "$output/compile-stderr.log"
python3 reports/native-qualification/javascript-search/check-after.py --source "$source" --output "$output" --compile-log "$output/compile.log"
cargo clippy --workspace --all-targets --all-features --locked -j4 -- -D warnings > "$output/clippy.log" 2>&1
echo complete
