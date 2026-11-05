#!/usr/bin/env bash
# Compare keystroke-to-results latency in the real FileEditor: current Rust
# search vs the old QuickJS/CodeMirror path.
#
#   scripts/perf/search-ui-latency.sh [OLD_REF]
#
# OLD_REF defaults to c8ec3de2, the rebased refactor's upstream parent.
# The old worktree is temporary; nothing in the current tree is modified.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
old_ref=${OLD_REF:-c8ec3de2}
profile=${PROFILE:-native-dev}
if [[ -n ${MONOCODE_SEARCH_LATENCY+x} ]]; then
  echo "search-ui-latency: unset MONOCODE_SEARCH_LATENCY to avoid timing log I/O" >&2
  exit 1
fi

if [[ $# -gt 0 && $1 != --* ]]; then
  old_ref=$1
  shift
fi
if [[ $# -gt 0 ]]; then echo "usage: $0 [OLD_REF]" >&2; exit 1; fi

if ! git -C "$root" rev-parse --verify --quiet "$old_ref^{commit}" >/dev/null; then
  echo "search-ui-latency: old ref not found: $old_ref" >&2
  exit 1
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/monocode-search-ui.XXXXXX")
worktree="$tmp/old"
cleanup() {
  git -C "$root" worktree remove --force "$worktree" >/dev/null 2>&1 || true
  if [[ ${KEEP:-0} == 1 ]]; then
    echo "search-ui-latency artifacts: $tmp" >&2
  else
    rm -rf "$tmp"
  fi
}
trap cleanup EXIT

git -C "$root" worktree add --detach "$worktree" "$old_ref" >/dev/null
cp "$root/crates/monocode-gpui/src/file_editor/latency_test.rs" \
  "$worktree/crates/monocode-gpui/src/file_editor/latency_test.rs"
python3 - "$worktree/crates/monocode-gpui/src/file_editor.rs" <<'PY'
import sys

path = sys.argv[1]
text = open(path).read()
needle = '#[cfg(test)]\n#[path = "file_editor/tests.rs"]\nmod parity_tests;\n'
if needle not in text:
    raise SystemExit("search-ui-latency: cannot find parity_tests module declaration")
replacement = needle + (
    '#[cfg(test)]\n#[path = "file_editor/latency_test.rs"]\nmod latency_test;\n'
)
open(path, "w").write(text.replace(needle, replacement, 1))
PY

compile() {
  local directory=$1 name=$2
  # Compile both snapshots before timing either, sharing dependency artifacts.
  # Copy the executables because Cargo may reuse the same artifact filename.
  local binary
  binary=$(cd "$directory" && CARGO_TARGET_DIR="$root/crates/monocode-gpui/target" \
    cargo test --locked --profile "$profile" --bin monocode-gpui \
      --no-run --message-format=json 2>"$tmp/build-$name.log" | \
    python3 -c 'import json, sys
for line in sys.stdin:
    item = json.loads(line)
    if item.get("reason") == "compiler-artifact" and item.get("executable"):
        print(item["executable"])')
  cp "$binary" "$tmp/$name-tests"
}
compile "$root/crates/monocode-gpui" new
compile "$worktree/crates/monocode-gpui" old
{
  echo "profile=$profile rounds=${SEARCH_LATENCY_ROUNDS:-20} warmups=2"
  echo "old=$(git -C "$root" rev-parse "$old_ref")"
  echo "new=$(git -C "$root" rev-parse HEAD) + working tree"
  rustc -Vv
  uname -a
} | tee "$tmp/environment.txt"

# Reverse ordering on the second pass to expose run-order effects.
for entry in new:1 old:1 old:2 new:2; do
  name=${entry%:*}
  pass=${entry#*:}
  echo "=== $name pass $pass ($profile) ==="
  "$tmp/$name-tests" search_keystroke_latency --ignored --nocapture --test-threads=1 \
    2>&1 | tee "$tmp/$name-$pass.log"
done
