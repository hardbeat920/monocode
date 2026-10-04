#!/usr/bin/env bash
set -euo pipefail
if command -v apt-get >/dev/null 2>&1; then
  # GitHub's Ubuntu images ship LLVM's libunwind-14-dev, which conflicts with
  # the libunwind-dev that libgstreamer1.0-dev needs.
  if [[ "${GITHUB_ACTIONS:-}" == true ]] && dpkg -s libunwind-14-dev >/dev/null 2>&1; then
    sudo apt-get remove -y libunwind-14-dev
  fi
  sudo apt-get update
  sudo apt-get install -y build-essential clang cmake pkg-config libclang-dev \
    libasound2-dev libssl-dev libfontconfig1-dev libfreetype6-dev libdbus-1-dev \
    libx11-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev libvulkan-dev \
    mesa-vulkan-drivers libudev-dev rpm patchelf file desktop-file-utils nasm libicu-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav \
    gstreamer1.0-alsa
elif command -v dnf >/dev/null 2>&1; then
  sudo dnf install -y gcc gcc-c++ clang clang-devel cmake pkgconf-pkg-config \
    alsa-lib-devel openssl-devel fontconfig-devel freetype-devel dbus-devel \
    libX11-devel libxcb-devel libxkbcommon-devel libxkbcommon-x11-devel \
    wayland-devel libXrandr-devel libXi-devel libXcursor-devel libXinerama-devel \
    vulkan-loader-devel mesa-vulkan-drivers systemd-devel rpm-build patchelf \
    file desktop-file-utils nasm libicu-devel gstreamer1-devel gstreamer1-plugins-base-devel \
    gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugin-libav
else
  echo "Install the native GPUI dependencies listed in packaging/README.md." >&2
  exit 1
fi
