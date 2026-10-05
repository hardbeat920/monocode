#!/bin/sh
set -eu
umask 077
VERSION=${1:?Usage: install-native-host.sh VERSION}
case "$VERSION" in *[!A-Za-z0-9.+-]*) echo "Invalid version" >&2; exit 1;; esac
RELEASE_BASE=${MONOCODE_HOST_RELEASE_BASE_URL:-https://github.com/hardbeat920/monocode/releases/download/v$VERSION}
case "$RELEASE_BASE" in https://*) ;; *) echo "Release URL must use HTTPS" >&2; exit 1;; esac
case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-gnu ;;
  *) echo "MonoCode Host supports macOS, Linux, and Windows." >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64|aarch64) arch=aarch64 ;;
  x86_64|amd64) arch=x86_64 ;;
  *) echo "This host architecture has no MonoCode release." >&2; exit 1 ;;
esac
target="$arch-$os"
installed="$HOME/.monocode-host/runtime/$VERSION/monocode-host"
if [ -x "$installed" ] && [ "$("$installed" --version)" = "$VERSION" ]; then
  exec "$installed" connect --json </dev/null
fi
archive="monocode-host_${VERSION}_${target}.tar.gz"
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT HUP INT TERM
download() {
  if command -v curl >/dev/null 2>&1; then
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$1" --output "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget --quiet --https-only "$1" -O "$2"
  else
    echo "Install curl or wget to download MonoCode Host." >&2
    exit 1
  fi
}
download "$RELEASE_BASE/$archive" "$temp/$archive"
download "$RELEASE_BASE/SHA256SUMS" "$temp/SHA256SUMS"
expected=$(awk -v name="$archive" '$2 == name { print $1 }' "$temp/SHA256SUMS")
if [ "${#expected}" -ne 64 ]; then
  echo "The release does not contain exactly one checksum for $archive." >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$temp/$archive" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$temp/$archive" | awk '{print $1}')
fi
if [ "$actual" != "$expected" ]; then
  echo "MonoCode Host download checksum did not match." >&2
  exit 1
fi
entries=$(tar -tzf "$temp/$archive")
kind=$(tar -tvzf "$temp/$archive" | cut -c 1)
if [ "$entries" != "monocode-host" ] || [ "$kind" != "-" ]; then
  echo "The MonoCode Host archive has unexpected files." >&2
  exit 1
fi
tar -xzf "$temp/$archive" -C "$temp"
chmod 700 "$temp/monocode-host"
if [ "$("$temp/monocode-host" --version)" != "$VERSION" ]; then
  echo "The downloaded host version does not match this desktop." >&2
  exit 1
fi
"$temp/monocode-host" connect --json </dev/null
