#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

home="$tmp/Home With Spaces"
binary="$tmp/MonoCode % test"
mkdir -p "$home/.local/share/applications"
printf '#!/bin/sh\nexit 0\n' >"$binary"
chmod +x "$binary"
printf 'keep\n' >"$home/.local/share/unrelated.desktop"

HOME="$home" USER="installer-test" MONOCODE_BINARY="$binary" \
  bash "$root/scripts/install-linux-desktop.sh"

launcher="$home/.local/share/applications/com.monocode.desktop.desktop"
test -x "$home/.local/bin/monocode"
test -f "$launcher"
test "$(cat "$home/.local/share/unrelated.desktop")" = "keep"
grep -F 'Exec="' "$launcher" >/dev/null
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$launcher"
fi

# Reinstall over the files managed by MonoCode, then remove only those files.
HOME="$home" USER="installer-test" MONOCODE_BINARY="$binary" \
  bash "$root/scripts/install-linux-desktop.sh"
HOME="$home" USER="installer-test" bash "$root/scripts/install-linux-desktop.sh" --uninstall
test ! -e "$home/.local/bin/monocode"
test ! -e "$launcher"
test "$(cat "$home/.local/share/unrelated.desktop")" = "keep"
