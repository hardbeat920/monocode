#!/usr/bin/env bash
set -euo pipefail

# Repack the Tauri-generated AppImage so it runs against the host's WebKitGTK
# stack instead of the Ubuntu-built copies linuxdeploy bundles.
#
# The bundled WebKitGTK/GLib/GStreamer libraries cannot create an EGL display
# against newer Mesa (EGL_BAD_PARAMETER, then WebKitWebProcess aborts), and any
# partial bundle is internally inconsistent on rolling distros (host
# GStreamer needs newer GLib, host gio needs newer libmount, ...). So the whole
# bundled library tree and the linuxdeploy GTK hook are dropped, and the
# AppImage requires webkit2gtk-4.1 on the host, like the .deb and .rpm do.
#
# This rewrite happens after `tauri build`. Linux currently sets
# bundle.createUpdaterArtifacts to false in tauri.linux.conf.json. If that is
# ever enabled, the .AppImage.sig Tauri writes is for the pre-repack file and
# must be regenerated after this script, or the updater will reject the image.
#
# Usage: scripts/repack-appimage.sh [path/to/MonoCode_x.y.z_ARCH.AppImage]

APPIMAGETOOL_VERSION="1.9.1"

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

arch="$(uname -m)"
# Digests from the appimagetool 1.9.1 release assets. Keep each architecture's
# tool in a separate cache entry and verify it before execution.
case "$arch" in
  x86_64) APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0" ;;
  aarch64) APPIMAGETOOL_SHA256="f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158" ;;
  *)
    echo "repack-appimage.sh only supports x86_64 and aarch64 hosts (found $arch)" >&2
    exit 1
    ;;
esac

appimage="$(readlink -f "$appimage")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

tools_dir="${XDG_CACHE_HOME:-$HOME/.cache}/monocode-build"
appimagetool="$tools_dir/appimagetool-$APPIMAGETOOL_VERSION-$arch.AppImage"
mkdir -p "$tools_dir"
if [[ ! -f "$appimagetool" ]] || ! sha256sum --quiet -c <<<"$APPIMAGETOOL_SHA256  $appimagetool" 2>/dev/null; then
  curl -fsSL --retry 3 -o "$appimagetool.part" \
    "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$arch.AppImage"
  if ! sha256sum --quiet -c <<<"$APPIMAGETOOL_SHA256  $appimagetool.part"; then
    echo "appimagetool checksum mismatch" >&2
    rm -f "$appimagetool.part"
    exit 1
  fi
  mv "$appimagetool.part" "$appimagetool"
fi
chmod +x "$appimagetool"

chmod +x "$appimage"
# Reuse the exact runtime Tauri shipped, then extract the payload.
offset="$(APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" --appimage-offset | tr -d '[:space:]')"
if ! [[ "$offset" =~ ^[0-9]+$ ]] || (( offset < 65536 )); then
  echo "Could not read AppImage runtime offset from $appimage" >&2
  exit 1
fi
head -c "$offset" "$appimage" > "$work/runtime"
(cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" --appimage-extract >/dev/null)
appdir="$work/squashfs-root"

if [[ ! -x "$appdir/usr/bin/monocode" ]]; then
  echo "Unexpected AppImage layout: usr/bin/monocode is missing" >&2
  exit 1
fi

# Drop every bundled library (WebKitGTK, its helper processes, GLib, GTK,
# GStreamer, ...) and the linuxdeploy GTK hook, which forces GDK_BACKEND=x11
# and points GStreamer/GTK/GIO module paths into the bundle.
rm -rf "$appdir/usr/lib" "$appdir/apprun-hooks" "$appdir/AppRun.wrapped" "$appdir/AppRun"

cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
# MonoCode AppImage entry point. The AppImage uses the host's WebKitGTK stack.
HERE="$(dirname "$(readlink -f "$0")")"

webkit_ok=
if [ -e /usr/lib/libwebkit2gtk-4.1.so.0 ] ||
   [ -e /usr/lib64/libwebkit2gtk-4.1.so.0 ] ||
   [ -e /usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0 ] ||
   [ -e /lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0 ] ||
   [ -e /usr/lib/aarch64-linux-gnu/libwebkit2gtk-4.1.so.0 ] ||
   [ -e /lib/aarch64-linux-gnu/libwebkit2gtk-4.1.so.0 ]; then
  webkit_ok=1
elif command -v ldconfig >/dev/null 2>&1 &&
     ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4\.1\.so\.0'; then
  webkit_ok=1
fi

if [ -z "$webkit_ok" ]; then
  # Warn rather than exit: NixOS ships ldconfig with an empty cache, and
  # WebKit lives in the Nix store instead of /usr/lib. The dynamic linker
  # still finds it when the package is installed.
  cat >&2 <<'EOF'
MonoCode needs WebKitGTK 4.1 on the host. If startup fails, install it:
  Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0
  Fedora:        sudo dnf install webkit2gtk4.1
  Arch:          sudo pacman -S webkit2gtk-4.1
EOF
fi

exec "$HERE/usr/bin/monocode" "$@"
APPRUN
chmod +x "$appdir/AppRun"

# Fail loudly if a future bundler change ships libraries somewhere else.
leftover="$(find "$appdir" \( -name '*.so' -o -name '*.so.*' \) -print -quit)"
if [[ -n "$leftover" ]]; then
  echo "Bundled shared library still present after repack: ${leftover#"$appdir"/}" >&2
  exit 1
fi

(cd "$work" && ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" --appimage-extract-and-run \
  --no-appstream --runtime-file "$work/runtime" "$appdir" "$work/out.AppImage" >/dev/null)

chmod +x "$work/out.AppImage"
mv "$work/out.AppImage" "$appimage"
echo "Repacked $appimage against host WebKitGTK ($(du -h "$appimage" | cut -f1))"
