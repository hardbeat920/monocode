#!/usr/bin/env bash
set -euo pipefail

# Hosted runners include unrelated third-party repositories. CI can select the
# distribution's own source file without editing the machine's apt config.
APT_SOURCES=()
if [ "$#" -gt 0 ]; then
  if [ "$#" -ne 2 ] || [ "$1" != "--sources" ] || [ ! -f "$2" ]; then
    echo "Usage: $0 [--sources EXISTING_SOURCE_FILE]" >&2
    exit 1
  fi
  APT_SOURCES=(-o "Dir::Etc::sourcelist=$2" -o "Dir::Etc::sourceparts=-")
fi

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

# The conditional expansions also support empty arrays with nounset on Bash 3.
${SUDO[@]+"${SUDO[@]}"} apt-get ${APT_SOURCES[@]+"${APT_SOURCES[@]}"} update
${SUDO[@]+"${SUDO[@]}"} env DEBIAN_FRONTEND=noninteractive apt-get ${APT_SOURCES[@]+"${APT_SOURCES[@]}"} install -y \
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
