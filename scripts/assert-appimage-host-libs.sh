#!/usr/bin/env bash
set -euo pipefail

# Fail if a MonoCode AppImage still ships bundled shared libraries or the
# linuxdeploy GTK hook. Used by CI and the release linux job after
# scripts/repack-appimage.sh has rewritten the Tauri bundle.
#
# Usage: scripts/assert-appimage-host-libs.sh [path/to/MonoCode_x.y.z_amd64.AppImage]

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if (( $# > 1 )); then
  echo "Usage: $0 [path/to/AppImage]" >&2
  exit 2
fi

if (( $# == 1 )); then
  appimage="$1"
else
  shopt -s nullglob
  candidates=("$repo_root"/target/release/bundle/appimage/*.AppImage)
  shopt -u nullglob
  if (( ${#candidates[@]} != 1 )); then
    echo "Expected exactly one AppImage under target/release/bundle/appimage, found ${#candidates[@]}" >&2
    exit 1
  fi
  appimage="${candidates[0]}"
fi

appimage="$(readlink -f "$appimage")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

chmod +x "$appimage"
(cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" --appimage-extract >/dev/null)
root="$work/squashfs-root"

webkit="$(find "$root" -name 'libwebkit2gtk-4.1.so.0' -print -quit)"
if [[ -n "$webkit" ]]; then
  echo "AppImage still contains libwebkit2gtk-4.1.so.0" >&2
  exit 1
fi

leftover="$(find "$root" \( -name '*.so' -o -name '*.so.*' \) -print)"
if [[ -n "$leftover" ]]; then
  echo "AppImage still contains bundled shared libraries:" >&2
  echo "$leftover" >&2
  exit 1
fi

if [[ -e "$root/apprun-hooks/linuxdeploy-plugin-gtk.sh" ]] ||
   grep -q 'linuxdeploy-plugin-gtk' "$root/AppRun"; then
  echo "AppImage still contains the linuxdeploy GTK hook" >&2
  exit 1
fi
