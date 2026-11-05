#!/usr/bin/env bash
# Compare the native Rust editor search against the old QuickJS/CodeMirror path.
#
#   scripts/perf/search-bench.sh [OLD_REF] [options for both runs]
#
# OLD_REF defaults to c8ec3de2, the rebased refactor's upstream parent.
# Options: --large --iterations N --filter TEXT
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
bench="$root/scripts/perf/search-bench"
old_ref=${OLD_REF:-c8ec3de2}

if [[ $# -gt 0 && $1 != --* ]]; then
  old_ref=$1
  shift
fi

if ! git -C "$root" rev-parse --verify --quiet "$old_ref^{commit}" >/dev/null; then
  echo "search-bench: old ref not found: $old_ref" >&2
  echo "Set OLD_REF to a commit that still contains the old native-editor.js bundle." >&2
  exit 1
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/monocode-search-bench.XXXXXX")
cleanup() {
  if [[ ${KEEP:-0} == 1 ]]; then
    echo "search-bench artifacts: $tmp" >&2
  else
    rm -rf "$tmp"
  fi
}
trap cleanup EXIT

bundle="$tmp/native-editor-old.js"
git -C "$root" show "$old_ref:crates/monocode-gpui/assets/native-editor.js" >"$bundle"
if ! grep -q 'monocodeSearch' "$bundle"; then
  echo "search-bench: $old_ref does not contain the old monocodeSearch bundle" >&2
  exit 1
fi

cargo build --quiet --locked --release --manifest-path "$bench/Cargo.toml"
export SEARCH_BASELINE=$(git -C "$root" rev-parse "$old_ref")
export SEARCH_CANDIDATE="$(git -C "$root" rev-parse HEAD) + working tree"
{
  git -C "$root" rev-parse HEAD
  rustc -Vv
  uname -a
  if [[ $(uname) == Darwin ]]; then sysctl -n machdep.cpu.brand_string; fi
} >"$tmp/environment.txt"

cargo run --quiet --locked --release --manifest-path "$bench/Cargo.toml" -- \
  verify --bundle "$bundle"
cargo run --quiet --locked --release --manifest-path "$bench/Cargo.toml" -- \
  run --bundle "$bundle" --json "$tmp/comparison.json" "$@" | tee "$tmp/comparison.txt"
