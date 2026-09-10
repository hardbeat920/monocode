#!/usr/bin/env bash
set -euo pipefail

if ! command -v apt-get >/dev/null 2>&1; then
  echo "This helper currently supports Ubuntu/Debian systems with apt-get." >&2
  exit 1
fi

if [ "$(id -u)" -eq 0 ]; then
  SUDO=()
elif command -v sudo >/dev/null 2>&1; then
  SUDO=(sudo)
else
  echo "sudo is required when not running as root." >&2
  exit 1
fi

"${SUDO[@]}" apt-get update
"${SUDO[@]}" env DEBIAN_FRONTEND=noninteractive apt-get install -y \
  build-essential \
  curl \
  file \
  libayatana-appindicator3-dev \
  libgtk-3-dev \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  librsvg2-dev \
  patchelf \
  wget
